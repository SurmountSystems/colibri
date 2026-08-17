//! rkyv duplex bridge over the process serve mux.
//!
//! # Architecture (honest)
//!
//! Host apps (GPUI, embed binaries) talk **only** colibri-sys APIs in-process.
//! [`EngineDuplex`] is the typed **app ↔ host** control plane: [`ClientFrame`] /
//! [`ServerFrame`] (rkyv, length-prefixed). Under the hood it drives
//! [`ServeClient`] line protocol on the C engine's **stdin/stdout**
//! (`SUBMIT` / `DATA` / `DONE` / visual lines).
//!
//! | Layer | What it is |
//! |-------|------------|
//! | App / UI | `ClientFrame` / `ServerFrame` via this type |
//! | This bridge | Translate frames ↔ mux calls |
//! | Engine | C subprocess, serve mux (not REST, not gRPC, not FFI) |
//!
//! Python `c/openai_server.py` is the HTTP OpenAI face of the **same** mux.
//! Native desktop that embeds colibri-sys does **not** need that HTTP server.

use crate::config::ColibriConfig;
use crate::engine::EngineHandle;
use crate::engine::serve::{GenerateRequest, ServeClient, ServeEvent};
use crate::error::Result;
use crate::model::ModelFamily;
use crate::stream::{ClientFrame, PROTOCOL_VERSION, ServerFrame};
use crate::visual::{Subscribe, VisualSnapshot};

/// Drive the engine through the rkyv duplex surface.
///
/// Construct from a live [`EngineHandle`] (or mock client). Call
/// [`EngineDuplex::handle`] / [`EngineDuplex::handle_with`] for each
/// [`ClientFrame`]. Progressive tokens arrive as [`ServerFrame::Token`] while
/// the mux streams `DATA` lines.
pub struct EngineDuplex {
    engine: EngineHandle,
    subscribe: Subscribe,
    model_id: String,
    last_profile_len: usize,
    last_hits_seq: u64,
}

impl EngineDuplex {
    /// Wrap a started engine. `model_id` is advertised in [`ServerFrame::Hello`].
    pub fn new(engine: EngineHandle, model_id: impl Into<String>) -> Self {
        Self {
            engine,
            subscribe: Subscribe::ALL,
            model_id: model_id.into(),
            last_profile_len: 0,
            last_hits_seq: 0,
        }
    }

    /// Wrap a mock or already-connected [`ServeClient`] (unit tests).
    pub fn from_client(
        client: ServeClient,
        config: ColibriConfig,
        family: ModelFamily,
        model_id: impl Into<String>,
    ) -> Self {
        Self::new(EngineHandle::from_client(client, config, family), model_id)
    }

    pub fn engine(&self) -> &EngineHandle {
        &self.engine
    }

    pub fn subscribe(&self) -> Subscribe {
        self.subscribe
    }

    pub fn set_subscribe(&mut self, mask: Subscribe) {
        self.subscribe = mask;
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Hello frame for session start (protocol version + identity).
    pub fn hello(&self) -> ServerFrame {
        let cfg = self.engine.config();
        let family = self.engine.family();
        ServerFrame::Hello {
            protocol_version: PROTOCOL_VERSION,
            model_id: self.model_id.clone(),
            engine_name: family.engine_basename().to_string(),
            kv_slots: cfg.kv_slots,
        }
    }

    /// Handle one client frame; collect all resulting server frames.
    ///
    /// Prefer [`handle_with`] when the host wants tokens as they stream (UI).
    pub fn handle(&mut self, frame: &ClientFrame) -> Result<Vec<ServerFrame>> {
        let mut out = Vec::new();
        self.handle_with(frame, |sf| {
            out.push(sf);
            Ok(())
        })?;
        Ok(out)
    }

    /// Handle one client frame, emitting server frames through `emit` immediately.
    ///
    /// On [`ClientFrame::Submit`], each mux `DATA` chunk becomes
    /// [`ServerFrame::Token`] before the final [`ServerFrame::Done`].
    pub fn handle_with<F>(&mut self, frame: &ClientFrame, mut emit: F) -> Result<()>
    where
        F: FnMut(ServerFrame) -> Result<()>,
    {
        match frame {
            ClientFrame::Ping { nonce } => {
                emit(ServerFrame::Pong { nonce: *nonce })?;
                Ok(())
            }
            ClientFrame::Subscribe { mask } => {
                self.subscribe = Subscribe(*mask);
                self.emit_visual_snapshot(&mut emit)?;
                Ok(())
            }
            // UI / duplex `req_id` is the mux SUBMIT id (explicit mapping).
            ClientFrame::Stop { req_id } => {
                self.engine.with_client(|c| c.stop_request(*req_id))?;
                Ok(())
            }
            ClientFrame::Cancel { req_id } => {
                self.engine.with_client(|c| c.cancel_request(*req_id))?;
                Ok(())
            }
            ClientFrame::Submit {
                req_id,
                slot,
                max_tokens,
                temperature,
                top_p,
                prompt,
                grammar,
            } => {
                let submit = SubmitArgs {
                    req_id: *req_id,
                    slot: *slot,
                    max_tokens: *max_tokens,
                    temperature: *temperature,
                    top_p: *top_p,
                    prompt,
                    grammar: grammar.as_deref(),
                };
                self.handle_submit(submit, &mut emit)
            }
        }
    }

    /// Refresh telemetry and emit subscribed visual frames (Hwinfo, Tiers, …).
    pub fn pump_visual(&mut self) -> Result<Vec<ServerFrame>> {
        let mut out = Vec::new();
        self.engine.pump_visual();
        self.emit_visual_snapshot(&mut |sf| {
            out.push(sf);
            Ok(())
        })?;
        Ok(out)
    }

    fn handle_submit<F>(&mut self, submit: SubmitArgs<'_>, emit: &mut F) -> Result<()>
    where
        F: FnMut(ServerFrame) -> Result<()>,
    {
        let req_id = submit.req_id;
        // Map duplex UI req_id → mux SUBMIT id explicitly (same value on the wire).
        let req = GenerateRequest {
            prompt: submit.prompt.to_string(),
            max_tokens: submit.max_tokens,
            temperature: submit.temperature,
            top_p: submit.top_p,
            cache_slot: submit.slot,
            grammar: submit.grammar.map(|g| g.to_string()),
            request_id: Some(req_id),
        };

        let stream_tokens = should_stream_tokens(self.subscribe);
        let mut saw_engine_error = false;

        let result = self.engine.generate_stream(req, |ev| {
            match ev {
                ServeEvent::Accept { prompt_tokens } => {
                    emit(ServerFrame::Accept {
                        req_id,
                        prompt_tokens: *prompt_tokens as u32,
                    })?;
                }
                ServeEvent::Data(bytes) if stream_tokens => {
                    emit(ServerFrame::Token {
                        req_id,
                        utf8: bytes.clone(),
                    })?;
                }
                ServeEvent::Data(_) => {}
                ServeEvent::Done(_) => {
                    // Terminal Done is emitted after generate_stream returns so
                    // visual telemetry absorbed post-turn is included next.
                }
                ServeEvent::Error(msg) => {
                    saw_engine_error = true;
                    emit(ServerFrame::Error {
                        req_id,
                        code: "engine".into(),
                        message: msg.clone(),
                    })?;
                }
            }
            Ok(())
        });

        match result {
            Ok(finished) => {
                emit(ServerFrame::Done {
                    req_id,
                    completion_tokens: finished.stats.completion_tokens,
                    tokens_per_second: finished.stats.tokens_per_second as f32,
                    cache_hit_percent: finished.stats.cache_hit_percent as f32,
                    rss_gb: finished.stats.rss_gb as f32,
                    prompt_tokens: finished.stats.prompt_tokens,
                    length_limited: finished.stats.length_limited,
                })?;
                self.emit_visual_snapshot(emit)?;
                Ok(())
            }
            Err(e) => {
                if !saw_engine_error {
                    let _ = emit(ServerFrame::Error {
                        req_id,
                        code: "engine".into(),
                        message: e.to_string(),
                    });
                }
                Err(e)
            }
        }
    }

    fn emit_visual_snapshot<F>(&mut self, emit: &mut F) -> Result<()>
    where
        F: FnMut(ServerFrame) -> Result<()>,
    {
        self.engine.pump_visual();
        let snap = self.engine.visual_snapshot();
        emit_from_snapshot(
            &snap,
            self.subscribe,
            &mut self.last_profile_len,
            &mut self.last_hits_seq,
            emit,
        )
    }
}

struct SubmitArgs<'a> {
    req_id: u64,
    slot: u32,
    max_tokens: u32,
    temperature: f32,
    top_p: f32,
    prompt: &'a str,
    grammar: Option<&'a str>,
}

fn should_stream_tokens(sub: Subscribe) -> bool {
    // Default ALL streams tokens. Explicit mask streams when TOKENS is set.
    // If only visual bits are set (no TOKENS), skip progressive Token frames.
    if sub.0 == 0 {
        return false;
    }
    if sub.0 == Subscribe::ALL.0 {
        return true;
    }
    sub.contains(Subscribe::TOKENS)
}

fn emit_from_snapshot<F>(
    snap: &VisualSnapshot,
    subscribe: Subscribe,
    last_profile_len: &mut usize,
    last_hits_seq: &mut u64,
    emit: &mut F,
) -> Result<()>
where
    F: FnMut(ServerFrame) -> Result<()>,
{
    if subscribe.contains(Subscribe::HW) || subscribe.0 == Subscribe::ALL.0 {
        if let Some(h) = &snap.hwinfo {
            emit(ServerFrame::Hwinfo {
                cores: h.cores,
                ram_total_gb: h.ram_total_gb as f32,
                ram_avail_gb: h.ram_avail_gb as f32,
                gpus: h.gpus,
                vram_total_gb: h.vram_total_gb as f32,
                cpu: h.cpu.clone(),
                gpu: h.gpu.clone(),
            })?;
        }
    }
    if subscribe.contains(Subscribe::VISUAL) || subscribe.0 == Subscribe::ALL.0 {
        if let Some(t) = &snap.tiers {
            emit(ServerFrame::Tiers {
                vram_n: t.vram,
                ram_n: t.ram,
                disk_n: t.disk,
                vram_gb: t.vram_gb as f32,
                ram_gb: t.ram_gb as f32,
            })?;
        }
        if let Some(m) = &snap.expert_map {
            emit(ServerFrame::ExpertMap {
                rows: m.rows as u16,
                cols: m.cols as u16,
                cells: m.cells.clone(),
            })?;
        }
        if let Some(h) = &snap.expert_hits {
            if h.seq != *last_hits_seq {
                *last_hits_seq = h.seq;
                emit(ServerFrame::ExpertHits {
                    rows: h.rows as u16,
                    cols: h.cols as u16,
                    bits: h.bits.clone(),
                    seq: h.seq,
                })?;
            }
        }
    }
    if subscribe.contains(Subscribe::PROFILE) || subscribe.0 == Subscribe::ALL.0 {
        let start = (*last_profile_len).min(snap.profile.len());
        for turn in &snap.profile[start..] {
            emit(ServerFrame::ProfTurn {
                seq: snap.profile_seq,
                wall_s: turn.wall_s as f32,
                prompt_tokens: turn.prompt_tokens,
                completion_tokens: turn.completion_tokens,
                expert_disk_s: turn.expert_disk_s as f32,
                expert_wait_s: turn.expert_wait_s as f32,
                expert_matmul_s: turn.expert_matmul_s as f32,
                attention_s: turn.attention_s as f32,
                lm_head_s: turn.lm_head_s as f32,
                forwards: turn.forwards,
            })?;
        }
        *last_profile_len = snap.profile.len();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::serve::{GenerateRequest, READY_SENTINEL, ServeClient};
    use std::io::{BufRead, BufReader, Read, Write};
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::thread;
    use std::time::Duration;

    struct ChanWriter(Option<Sender<Vec<u8>>>);
    struct ChanReader {
        rx: Receiver<Vec<u8>>,
        buf: Vec<u8>,
        pos: usize,
    }

    impl Write for ChanWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0
                .as_ref()
                .unwrap()
                .send(buf.to_vec())
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::BrokenPipe, e))?;
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Drop for ChanWriter {
        fn drop(&mut self) {
            self.0.take();
        }
    }

    impl Read for ChanReader {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            if self.pos >= self.buf.len() {
                match self.rx.recv() {
                    Ok(c) => {
                        self.buf = c;
                        self.pos = 0;
                    }
                    Err(_) => return Ok(0),
                }
            }
            let n = (self.buf.len() - self.pos).min(out.len());
            out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
            self.pos += n;
            Ok(n)
        }
    }

    fn chan_pipe() -> (ChanWriter, ChanReader) {
        let (tx, rx) = mpsc::channel();
        (
            ChanWriter(Some(tx)),
            ChanReader {
                rx,
                buf: Vec::new(),
                pos: 0,
            },
        )
    }

    fn mock_engine_thread(
        eng_w: ChanWriter,
        eng_r: ChanReader,
        tokens: &[&[u8]],
    ) -> thread::JoinHandle<()> {
        let tokens: Vec<Vec<u8>> = tokens.iter().map(|t| t.to_vec()).collect();
        thread::spawn(move || {
            let mut eng_w = eng_w;
            let mut eng_r = BufReader::new(eng_r);
            eng_w.write_all(READY_SENTINEL).unwrap();
            eng_w.write_all(b"\n").unwrap();
            eng_w
                .write_all(b"HWINFO 8 32.0 16.0 1 24.0 TestCPU|TestGPU\n")
                .unwrap();
            eng_w.write_all(b"TIERS 2 10 90 4.0 1.5\n").unwrap();
            eng_w.flush().unwrap();

            let mut line = String::new();
            eng_r.read_line(&mut line).unwrap();
            assert!(line.starts_with("SUBMIT "), "got {line}");
            let parts: Vec<&str> = line.split_whitespace().collect();
            let id = parts[1].to_string();
            let nbytes: usize = parts[3].parse().unwrap();
            let mut payload = vec![0u8; nbytes];
            eng_r.read_exact(&mut payload).unwrap();
            let mut nl = [0u8; 1];
            eng_r.read_exact(&mut nl).unwrap();

            writeln!(eng_w, "ACCEPT {id} 5").unwrap();
            for tok in &tokens {
                writeln!(eng_w, "DATA {id} {}", tok.len()).unwrap();
                eng_w.write_all(tok).unwrap();
                eng_w.write_all(b"\n").unwrap();
            }
            writeln!(eng_w, "DONE {id} STAT 2 20.0 10.0 0.5 5 0").unwrap();
            eng_w.flush().unwrap();
        })
    }

    #[test]
    fn duplex_ping_pong() {
        let (host_w, _eng_r) = chan_pipe();
        let (eng_w, host_r) = chan_pipe();
        let eng = thread::spawn(move || {
            let mut eng_w = eng_w;
            eng_w.write_all(READY_SENTINEL).unwrap();
            eng_w.write_all(b"\n").unwrap();
            eng_w.flush().unwrap();
            // idle until drop
            thread::sleep(Duration::from_millis(200));
        });
        let client = ServeClient::from_pipes(None, Box::new(host_w), Box::new(host_r)).unwrap();
        let mut duplex = EngineDuplex::from_client(
            client,
            ColibriConfig::default(),
            ModelFamily::Glm,
            "test-model",
        );
        let frames = duplex.handle(&ClientFrame::Ping { nonce: 42 }).unwrap();
        assert!(matches!(
            frames.as_slice(),
            [ServerFrame::Pong { nonce: 42 }]
        ));
        let hello = duplex.hello();
        match hello {
            ServerFrame::Hello {
                protocol_version,
                model_id,
                engine_name,
                kv_slots,
            } => {
                assert_eq!(protocol_version, PROTOCOL_VERSION);
                assert_eq!(model_id, "test-model");
                assert_eq!(engine_name, "colibri");
                assert_eq!(kv_slots, 1);
            }
            other => panic!("unexpected {other:?}"),
        }
        drop(duplex);
        let _ = eng.join();
    }

    #[test]
    fn duplex_submit_streams_tokens() {
        let (host_w, eng_r) = chan_pipe();
        let (eng_w, host_r) = chan_pipe();
        let eng = mock_engine_thread(eng_w, eng_r, &[b"hel", b"lo"]);

        let client = ServeClient::from_pipes(None, Box::new(host_w), Box::new(host_r)).unwrap();
        thread::sleep(Duration::from_millis(30));
        let mut duplex = EngineDuplex::from_client(
            client,
            ColibriConfig::default().kv_slots(2),
            ModelFamily::Glm,
            "glm-tiny",
        );

        let mut streamed = Vec::new();
        duplex
            .handle_with(
                &ClientFrame::Submit {
                    req_id: 7,
                    slot: 0,
                    max_tokens: 16,
                    temperature: 0.7,
                    top_p: 0.9,
                    prompt: "hi".into(),
                    grammar: None,
                },
                |sf| {
                    streamed.push(sf);
                    Ok(())
                },
            )
            .unwrap();

        // Accept, Token, Token, Done, plus visual (Hwinfo/Tiers under ALL)
        let tokens: Vec<_> = streamed
            .iter()
            .filter_map(|f| match f {
                ServerFrame::Token { req_id, utf8 } => {
                    assert_eq!(*req_id, 7);
                    Some(String::from_utf8_lossy(utf8).into_owned())
                }
                _ => None,
            })
            .collect();
        assert_eq!(tokens, vec!["hel".to_string(), "lo".to_string()]);

        assert!(
            streamed.iter().any(|f| matches!(
                f,
                ServerFrame::Accept {
                    req_id: 7,
                    prompt_tokens: 5
                }
            )),
            "missing Accept: {streamed:?}"
        );
        assert!(
            streamed.iter().any(|f| matches!(
                f,
                ServerFrame::Done {
                    req_id: 7,
                    completion_tokens: 2,
                    ..
                }
            )),
            "missing Done: {streamed:?}"
        );
        assert!(
            streamed
                .iter()
                .any(|f| matches!(f, ServerFrame::Hwinfo { .. })),
            "missing Hwinfo under Subscribe::ALL"
        );
        assert!(
            streamed
                .iter()
                .any(|f| matches!(f, ServerFrame::Tiers { .. })),
            "missing Tiers under Subscribe::ALL"
        );

        eng.join().unwrap();
    }

    #[test]
    fn duplex_subscribe_mask_skips_tokens_when_no_token_bit() {
        let (host_w, eng_r) = chan_pipe();
        let (eng_w, host_r) = chan_pipe();
        let eng = mock_engine_thread(eng_w, eng_r, &[b"x"]);

        let client = ServeClient::from_pipes(None, Box::new(host_w), Box::new(host_r)).unwrap();
        thread::sleep(Duration::from_millis(30));
        let mut duplex =
            EngineDuplex::from_client(client, ColibriConfig::default(), ModelFamily::Glm, "m");
        duplex
            .handle(&ClientFrame::Subscribe {
                mask: Subscribe::HW.0,
            })
            .unwrap();

        let frames = duplex
            .handle(&ClientFrame::Submit {
                req_id: 1,
                slot: 0,
                max_tokens: 4,
                temperature: 1.0,
                top_p: 1.0,
                prompt: "p".into(),
                grammar: None,
            })
            .unwrap();

        assert!(
            !frames
                .iter()
                .any(|f| matches!(f, ServerFrame::Token { .. })),
            "tokens should be suppressed when TOKENS bit off: {frames:?}"
        );
        assert!(frames.iter().any(|f| matches!(f, ServerFrame::Done { .. })));
        eng.join().unwrap();
    }

    #[test]
    fn generate_stream_callback_sees_data() {
        let (host_w, eng_r) = chan_pipe();
        let (eng_w, host_r) = chan_pipe();
        let eng = mock_engine_thread(eng_w, eng_r, &[b"a", b"b"]);
        let client = ServeClient::from_pipes(None, Box::new(host_w), Box::new(host_r)).unwrap();
        let mut chunks = Vec::new();
        let result = client
            .generate_stream(
                &GenerateRequest {
                    prompt: "p".into(),
                    max_tokens: 8,
                    ..Default::default()
                },
                |ev| {
                    if let ServeEvent::Data(b) = ev {
                        chunks.push(b.clone());
                    }
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(result.text, "ab");
        assert_eq!(chunks, vec![b"a".to_vec(), b"b".to_vec()]);
        eng.join().unwrap();
    }

    /// Duplex Cancel must write CANCEL (not STOP) with the UI req_id as mux id.
    #[test]
    fn duplex_cancel_writes_cancel_with_ui_req_id() {
        let (host_w, eng_r) = chan_pipe();
        let (eng_w, host_r) = chan_pipe();
        let eng = thread::spawn(move || {
            let mut eng_w = eng_w;
            let mut eng_r = BufReader::new(eng_r);
            eng_w.write_all(READY_SENTINEL).unwrap();
            eng_w.write_all(b"\n").unwrap();
            eng_w.flush().unwrap();
            let mut line = String::new();
            eng_r.read_line(&mut line).unwrap();
            assert_eq!(line, "CANCEL 77\n", "got {line:?}");
        });

        let client = ServeClient::from_pipes(None, Box::new(host_w), Box::new(host_r)).unwrap();
        let mut duplex =
            EngineDuplex::from_client(client, ColibriConfig::default(), ModelFamily::Glm, "m");
        duplex.handle(&ClientFrame::Cancel { req_id: 77 }).unwrap();
        drop(duplex);
        eng.join().unwrap();
    }

    /// Duplex Stop writes STOP with the same id the UI uses (explicit mapping).
    #[test]
    fn duplex_stop_writes_stop_with_ui_req_id() {
        let (host_w, eng_r) = chan_pipe();
        let (eng_w, host_r) = chan_pipe();
        let eng = thread::spawn(move || {
            let mut eng_w = eng_w;
            let mut eng_r = BufReader::new(eng_r);
            eng_w.write_all(READY_SENTINEL).unwrap();
            eng_w.write_all(b"\n").unwrap();
            eng_w.flush().unwrap();
            let mut line = String::new();
            eng_r.read_line(&mut line).unwrap();
            assert_eq!(line, "STOP 88\n", "got {line:?}");
        });

        let client = ServeClient::from_pipes(None, Box::new(host_w), Box::new(host_r)).unwrap();
        let mut duplex =
            EngineDuplex::from_client(client, ColibriConfig::default(), ModelFamily::Glm, "m");
        duplex.handle(&ClientFrame::Stop { req_id: 88 }).unwrap();
        drop(duplex);
        eng.join().unwrap();
    }

    /// Submit uses UI req_id on the SUBMIT line (not a separate auto id).
    #[test]
    fn duplex_submit_maps_ui_req_id_to_mux_submit() {
        let (host_w, eng_r) = chan_pipe();
        let (eng_w, host_r) = chan_pipe();
        let eng = thread::spawn(move || {
            let mut eng_w = eng_w;
            let mut eng_r = BufReader::new(eng_r);
            eng_w.write_all(READY_SENTINEL).unwrap();
            eng_w.write_all(b"\n").unwrap();
            eng_w.flush().unwrap();

            let mut line = String::new();
            eng_r.read_line(&mut line).unwrap();
            assert!(
                line.starts_with("SUBMIT 99 "),
                "UI req_id must be mux id, got {line:?}"
            );
            let parts: Vec<&str> = line.split_whitespace().collect();
            let nbytes: usize = parts[3].parse().unwrap();
            let mut payload = vec![0u8; nbytes];
            eng_r.read_exact(&mut payload).unwrap();
            let mut nl = [0u8; 1];
            eng_r.read_exact(&mut nl).unwrap();
            writeln!(eng_w, "ACCEPT 99 1").unwrap();
            writeln!(eng_w, "DONE 99 STAT 0 0.0 0.0 0.0 1 0").unwrap();
            eng_w.flush().unwrap();
        });

        let client = ServeClient::from_pipes(None, Box::new(host_w), Box::new(host_r)).unwrap();
        thread::sleep(Duration::from_millis(20));
        let mut duplex =
            EngineDuplex::from_client(client, ColibriConfig::default(), ModelFamily::Glm, "m");
        let frames = duplex
            .handle(&ClientFrame::Submit {
                req_id: 99,
                slot: 0,
                max_tokens: 4,
                temperature: 1.0,
                top_p: 1.0,
                prompt: "hi".into(),
                grammar: None,
            })
            .unwrap();
        assert!(
            frames
                .iter()
                .any(|f| matches!(f, ServerFrame::Done { req_id: 99, .. })),
            "missing Done with UI req_id: {frames:?}"
        );
        eng.join().unwrap();
    }

    /// Duplex Submit maps slot, max_tokens, temperature, and GBNF grammar onto
    /// the mux SUBMIT header + optional grammar payload.
    #[test]
    fn duplex_submit_forwards_slot_temp_max_tokens_and_grammar() {
        let (host_w, eng_r) = chan_pipe();
        let (eng_w, host_r) = chan_pipe();
        let gbnf = "root ::= \"ok\"";
        let eng = thread::spawn(move || {
            let mut eng_w = eng_w;
            let mut eng_r = BufReader::new(eng_r);
            eng_w.write_all(READY_SENTINEL).unwrap();
            eng_w.write_all(b"\n").unwrap();
            eng_w.flush().unwrap();

            let mut line = String::new();
            eng_r.read_line(&mut line).unwrap();
            // SUBMIT <id> <slot> <prompt_bytes> <max_tokens> <temp> <top_p> [grammar_bytes]
            let parts: Vec<&str> = line.split_whitespace().collect();
            assert!(
                line.starts_with("SUBMIT 11 "),
                "expected SUBMIT 11, got {line:?}"
            );
            assert_eq!(parts[2], "3", "cache_slot / slot: {line:?}");
            assert_eq!(parts[3], "2", "prompt byte len: {line:?}"); // "hi"
            assert_eq!(parts[4], "64", "max_tokens: {line:?}");
            let temp: f32 = parts[5].parse().expect("temp");
            assert!((temp - 0.25).abs() < 1e-5, "temperature: {line:?}");
            assert_eq!(parts.len(), 8, "grammar length field expected: {line:?}");
            let gbytes: usize = parts[7].parse().expect("grammar bytes");
            assert_eq!(gbytes, gbnf.len());

            let mut payload = vec![0u8; 2 + gbytes];
            eng_r.read_exact(&mut payload).unwrap();
            let mut nl = [0u8; 1];
            eng_r.read_exact(&mut nl).unwrap();
            assert_eq!(&payload[..2], b"hi");
            assert_eq!(&payload[2..], gbnf.as_bytes());

            writeln!(eng_w, "ACCEPT 11 1").unwrap();
            writeln!(eng_w, "DONE 11 STAT 0 0.0 0.0 0.0 1 0").unwrap();
            eng_w.flush().unwrap();
        });

        let client = ServeClient::from_pipes(None, Box::new(host_w), Box::new(host_r)).unwrap();
        thread::sleep(Duration::from_millis(20));
        let mut duplex =
            EngineDuplex::from_client(client, ColibriConfig::default(), ModelFamily::Glm, "m");
        let frames = duplex
            .handle(&ClientFrame::Submit {
                req_id: 11,
                slot: 3,
                max_tokens: 64,
                temperature: 0.25,
                top_p: 0.9,
                prompt: "hi".into(),
                grammar: Some(gbnf.into()),
            })
            .unwrap();
        assert!(
            frames
                .iter()
                .any(|f| matches!(f, ServerFrame::Done { req_id: 11, .. })),
            "missing Done: {frames:?}"
        );
        eng.join().unwrap();
    }

    /// Empty / None grammar must not add a 7th header field (mux payload is prompt only).
    #[test]
    fn duplex_submit_omits_grammar_field_when_none() {
        let (host_w, eng_r) = chan_pipe();
        let (eng_w, host_r) = chan_pipe();
        let eng = thread::spawn(move || {
            let mut eng_w = eng_w;
            let mut eng_r = BufReader::new(eng_r);
            eng_w.write_all(READY_SENTINEL).unwrap();
            eng_w.write_all(b"\n").unwrap();
            eng_w.flush().unwrap();

            let mut line = String::new();
            eng_r.read_line(&mut line).unwrap();
            let parts: Vec<&str> = line.split_whitespace().collect();
            // SUBMIT id slot nbytes max_tokens temp top_p  → 7 fields, no grammar len
            assert_eq!(parts.len(), 7, "no grammar field expected: {line:?}");
            let nbytes: usize = parts[3].parse().unwrap();
            let mut payload = vec![0u8; nbytes];
            eng_r.read_exact(&mut payload).unwrap();
            let mut nl = [0u8; 1];
            eng_r.read_exact(&mut nl).unwrap();
            writeln!(eng_w, "ACCEPT {} 1", parts[1]).unwrap();
            writeln!(eng_w, "DONE {} STAT 0 0.0 0.0 0.0 1 0", parts[1]).unwrap();
            eng_w.flush().unwrap();
        });

        let client = ServeClient::from_pipes(None, Box::new(host_w), Box::new(host_r)).unwrap();
        thread::sleep(Duration::from_millis(20));
        let mut duplex =
            EngineDuplex::from_client(client, ColibriConfig::default(), ModelFamily::Glm, "m");
        duplex
            .handle(&ClientFrame::Submit {
                req_id: 1,
                slot: 0,
                max_tokens: 8,
                temperature: 1.0,
                top_p: 1.0,
                prompt: "x".into(),
                grammar: None,
            })
            .unwrap();
        eng.join().unwrap();
    }
}
