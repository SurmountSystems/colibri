//! Serve mux client (line protocol over engine stdin/stdout).
//!
//! Port of framing from `c/openai_server.py` (`Engine` dispatcher) and
//! `docs/serve_protocol.md`. Production path: `SERVE=1` + `SERVE_BATCH=1`.
//!
//! Handshake: wait for `\x01\x01READY\x01\x01`. Then SUBMIT / DATA / DONE.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::config::EnvMap;
use crate::error::{Error, Result};
use crate::model::ModelFamily;
use crate::visual::{HwinfoSnap, ProfileTurn, TiersSnap};

/// READY sentinel (bytes before the trailing newline readline consumes).
pub const READY_SENTINEL: &[u8] = b"\x01\x01READY\x01\x01";

/// Generation request for one mux SUBMIT.
#[derive(Debug, Clone)]
pub struct GenerateRequest {
    pub prompt: String,
    pub max_tokens: u32,
    pub temperature: f32,
    pub top_p: f32,
    pub cache_slot: u32,
    pub grammar: Option<String>,
    /// Mux SUBMIT id. When `None`, [`ServeClient`] allocates from an internal
    /// counter. When `Some`, that exact non-zero id is written on the wire
    /// (UI / duplex `req_id` mapping). Zero is rejected.
    pub request_id: Option<u64>,
}

impl Default for GenerateRequest {
    fn default() -> Self {
        Self {
            prompt: String::new(),
            max_tokens: 128,
            temperature: 1.0,
            top_p: 1.0,
            cache_slot: 0,
            grammar: None,
            request_id: None,
        }
    }
}

/// In-flight generate after SUBMIT is written; wait with [`InFlightGenerate::recv_loop`].
///
/// Holding this does **not** require locking [`ServeClient`] for the whole stream,
/// so a concurrent thread may call [`ServeClient::stop_request`] /
/// [`ServeClient::cancel_request`] on the same client.
#[derive(Debug)]
pub struct InFlightGenerate {
    request_id: u64,
    rx: Receiver<ServeEvent>,
}

impl InFlightGenerate {
    /// Mux id written on the SUBMIT line (and used by STOP / CANCEL).
    pub fn request_id(&self) -> u64 {
        self.request_id
    }

    /// Block until DONE or Error, invoking `on_event` for progressive events.
    pub fn recv_loop<F>(self, mut on_event: F) -> Result<GenerateResult>
    where
        F: FnMut(&ServeEvent) -> Result<()>,
    {
        let mut text = String::new();
        let mut prompt_tokens_accept = None;
        loop {
            let event = self
                .rx
                .recv()
                .map_err(|_| Error::engine("engine channel closed"))?;
            match &event {
                ServeEvent::Accept { prompt_tokens } => {
                    prompt_tokens_accept = Some(*prompt_tokens);
                    on_event(&event)?;
                }
                ServeEvent::Data(bytes) => {
                    text.push_str(&String::from_utf8_lossy(bytes));
                    on_event(&event)?;
                }
                ServeEvent::Done(stats) => {
                    let result = GenerateResult {
                        text,
                        stats: stats.clone(),
                        prompt_tokens_accept,
                    };
                    on_event(&event)?;
                    return Ok(result);
                }
                ServeEvent::Error(msg) => {
                    let _ = on_event(&event);
                    return Err(Error::engine(msg.clone()));
                }
            }
        }
    }
}

/// Completion statistics from DONE STAT fields.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DoneStats {
    pub completion_tokens: u64,
    pub tokens_per_second: f64,
    pub cache_hit_percent: f64,
    pub rss_gb: f64,
    pub prompt_tokens: u64,
    pub length_limited: bool,
}

/// Result of a generate call.
#[derive(Debug, Clone)]
pub struct GenerateResult {
    pub text: String,
    pub stats: DoneStats,
    pub prompt_tokens_accept: Option<u64>,
}

// HwinfoSnap / TiersSnap live in `crate::visual` (shared with function APIs).

/// Events from the dispatcher.
#[derive(Debug)]
pub enum ServeEvent {
    Data(Vec<u8>),
    Accept { prompt_tokens: u64 },
    Done(DoneStats),
    Error(String),
}

#[derive(Default)]
struct Telemetry {
    tiers: Option<TiersSnap>,
    hwinfo: Option<HwinfoSnap>,
    emap_hex: Option<(u32, u32, String)>,
    hits_hex: Option<String>,
    hits_seq: u64,
    profile: Vec<ProfileTurn>,
    profile_seq: u64,
    dispatcher_error: Option<String>,
}

struct Shared {
    pending: Mutex<HashMap<String, Sender<ServeEvent>>>,
    telemetry: Mutex<Telemetry>,
    closed: Mutex<bool>,
}

/// Connected mux client (child process or mock pipes).
pub struct ServeClient {
    child: Option<Child>,
    stdin: Mutex<Box<dyn Write + Send>>,
    shared: Arc<Shared>,
    next_id: Mutex<u64>,
    _dispatcher: Option<JoinHandle<()>>,
}

impl ServeClient {
    /// Spawn engine binary with SERVE_BATCH env and wait for READY.
    pub fn spawn(
        executable: &Path,
        env: &EnvMap,
        cap: Option<u32>,
        family: ModelFamily,
    ) -> Result<Self> {
        let mut cmd = Command::new(executable);
        let cap_arg = match (cap, family) {
            (Some(c), _) => c.to_string(),
            (None, ModelFamily::Glm) | (None, ModelFamily::Olmoe) => "0".into(),
            (None, _) => "8".into(),
        };
        cmd.arg(&cap_arg)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        env.apply_to_command(&mut cmd);
        cmd.env("SERVE", "1");
        cmd.env("SERVE_BATCH", "1");
        if env.get("COLI_NO_OMP_TUNE").is_none() {
            cmd.env("COLI_NO_OMP_TUNE", "1");
        }
        // Demote only the engine child; UI/host stays at default priority.
        crate::process_priority::apply_low_compute_priority(&mut cmd);

        let mut child = cmd
            .spawn()
            .map_err(|e| Error::engine(format!("failed to spawn {}: {e}", executable.display())))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error::engine("engine stdin missing"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::engine("engine stdout missing"))?;
        Self::from_pipes(Some(child), Box::new(stdin), Box::new(stdout))
    }

    /// Build a client from raw pipes (mock engines in tests).
    pub fn from_pipes(
        child: Option<Child>,
        stdin: Box<dyn Write + Send>,
        stdout: Box<dyn Read + Send>,
    ) -> Result<Self> {
        let mut reader = BufReader::new(stdout);
        wait_ready(&mut reader)?;

        let shared = Arc::new(Shared {
            pending: Mutex::new(HashMap::new()),
            telemetry: Mutex::new(Telemetry::default()),
            closed: Mutex::new(false),
        });
        let shared_disp = Arc::clone(&shared);
        let dispatcher = thread::Builder::new()
            .name("colibri-serve-stdout".into())
            .spawn(move || dispatch_loop(reader, shared_disp))
            .map_err(Error::Io)?;

        Ok(Self {
            child,
            stdin: Mutex::new(stdin),
            shared,
            next_id: Mutex::new(1),
            _dispatcher: Some(dispatcher),
        })
    }

    pub fn tiers(&self) -> Option<TiersSnap> {
        self.shared.telemetry.lock().tiers.clone()
    }

    pub fn hwinfo(&self) -> Option<HwinfoSnap> {
        self.shared.telemetry.lock().hwinfo.clone()
    }

    pub fn emap_hex(&self) -> Option<(u32, u32, String)> {
        self.shared.telemetry.lock().emap_hex.clone()
    }

    pub fn hits_hex(&self) -> Option<String> {
        self.shared.telemetry.lock().hits_hex.clone()
    }

    pub fn hits_seq(&self) -> u64 {
        self.shared.telemetry.lock().hits_seq
    }

    pub fn profile(&self) -> Vec<ProfileTurn> {
        self.shared.telemetry.lock().profile.clone()
    }

    pub fn profile_seq(&self) -> u64 {
        self.shared.telemetry.lock().profile_seq
    }

    /// SUBMIT and collect DATA until DONE (blocking full result).
    pub fn generate(&self, req: &GenerateRequest) -> Result<GenerateResult> {
        self.generate_stream(req, |_| Ok(()))
    }

    /// Write SUBMIT and return an [`InFlightGenerate`] whose [`InFlightGenerate::recv_loop`]
    /// collects events until DONE. Hosts that must allow concurrent STOP/CANCEL should
    /// call this, drop any outer locks, then run `recv_loop`.
    pub fn begin_generate(&self, req: &GenerateRequest) -> Result<InFlightGenerate> {
        if *self.shared.closed.lock() {
            return Err(Error::engine("colibri engine is shutting down"));
        }
        if let Some(err) = self.shared.telemetry.lock().dispatcher_error.clone() {
            return Err(Error::engine(err));
        }

        let request_id_num = match req.request_id {
            Some(0) => {
                return Err(Error::invalid(
                    "request id must be non-zero (serve mux SUBMIT id)",
                ));
            }
            Some(id) => {
                // Keep the auto-allocator past any explicit id so later None
                // allocations do not collide with a still-inflight explicit id.
                let mut next = self.next_id.lock();
                if id >= *next {
                    *next = id.saturating_add(1);
                }
                id
            }
            None => {
                let mut id = self.next_id.lock();
                let v = *id;
                *id = id.saturating_add(1);
                v
            }
        };
        let request_id = request_id_num.to_string();

        let (tx, rx): (Sender<ServeEvent>, Receiver<ServeEvent>) = mpsc::channel();
        {
            let mut pending = self.shared.pending.lock();
            // Protocol: ids must be unique among in-flight requests.
            if pending.contains_key(&request_id) {
                return Err(Error::invalid(format!(
                    "request id {request_id} is already in flight"
                )));
            }
            pending.insert(request_id.clone(), tx);
        }

        let payload = req.prompt.as_bytes();
        if payload.contains(&0) {
            self.shared.pending.lock().remove(&request_id);
            return Err(Error::invalid("NUL bytes are not supported in prompts"));
        }
        let gpayload = req
            .grammar
            .as_ref()
            .map(|g| g.as_bytes().to_vec())
            .unwrap_or_default();
        if gpayload.contains(&0) {
            self.shared.pending.lock().remove(&request_id);
            return Err(Error::invalid("NUL bytes are not supported in grammars"));
        }

        let mut header = format!(
            "SUBMIT {request_id} {} {} {} {:.8} {:.8}",
            req.cache_slot,
            payload.len(),
            req.max_tokens,
            req.temperature,
            req.top_p
        );
        if !gpayload.is_empty() {
            header.push_str(&format!(" {}", gpayload.len()));
        }
        header.push('\n');

        let write_ok = (|| -> std::io::Result<()> {
            let mut stdin = self.stdin.lock();
            stdin.write_all(header.as_bytes())?;
            stdin.write_all(payload)?;
            if !gpayload.is_empty() {
                stdin.write_all(&gpayload)?;
            }
            stdin.write_all(b"\n")?;
            stdin.flush()?;
            Ok(())
        })();
        if let Err(e) = write_ok {
            // Mirror NUL paths: never leave a dead Sender in pending after insert.
            self.shared.pending.lock().remove(&request_id);
            return Err(Error::engine(e.to_string()));
        }

        Ok(InFlightGenerate {
            request_id: request_id_num,
            rx,
        })
    }

    /// SUBMIT with progressive event callback until DONE.
    ///
    /// Invokes `on_event` for each [`ServeEvent::Accept`] and
    /// [`ServeEvent::Data`] as the mux produces them (true streaming for hosts
    /// that want token-by-token UI). [`ServeEvent::Done`] / error ends the call;
    /// the final [`GenerateResult`] still aggregates full text + stats.
    ///
    /// Prefer [`begin_generate`] + `recv_loop` when an outer mutex must not be
    /// held across the receive loop (so STOP/CANCEL can run concurrently).
    pub fn generate_stream<F>(&self, req: &GenerateRequest, on_event: F) -> Result<GenerateResult>
    where
        F: FnMut(&ServeEvent) -> Result<()>,
    {
        self.begin_generate(req)?.recv_loop(on_event)
    }

    /// Send `STOP <id>` (graceful end → normal DONE path; stats + KV kept).
    pub fn stop_request(&self, request_id: u64) -> Result<()> {
        let mut stdin = self.stdin.lock();
        writeln!(stdin, "STOP {request_id}").map_err(|e| Error::engine(e.to_string()))?;
        stdin.flush().map_err(|e| Error::engine(e.to_string()))?;
        Ok(())
    }

    /// Send `CANCEL <id>` (abort → engine `ERROR <id> CANCELLED`).
    pub fn cancel_request(&self, request_id: u64) -> Result<()> {
        let mut stdin = self.stdin.lock();
        writeln!(stdin, "CANCEL {request_id}").map_err(|e| Error::engine(e.to_string()))?;
        stdin.flush().map_err(|e| Error::engine(e.to_string()))?;
        Ok(())
    }

    /// Tear down the engine: mark closed, wake in-flight waiters, kill child.
    ///
    /// Pending generates are failed with "colibri engine is shutting down" so
    /// unlocked `recv_loop` callers do not block forever after process stop.
    /// This is process teardown (not mux `STOP <id>`).
    pub fn shutdown(&mut self) -> Result<()> {
        *self.shared.closed.lock() = true;
        // Drain pending before killing so mid-stream generate_stream wakes.
        {
            let pending: Vec<_> = self
                .shared
                .pending
                .lock()
                .drain()
                .map(|(_, tx)| tx)
                .collect();
            for tx in pending {
                let _ = tx.send(ServeEvent::Error("colibri engine is shutting down".into()));
            }
        }
        {
            let mut stdin = self.stdin.lock();
            let _ = stdin.flush();
        }
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        Ok(())
    }
}

impl Drop for ServeClient {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn wait_ready(reader: &mut BufReader<Box<dyn Read + Send>>) -> Result<()> {
    let mut line = Vec::new();
    loop {
        line.clear();
        let n = reader
            .read_until(b'\n', &mut line)
            .map_err(|e| Error::protocol(format!("waiting for READY: {e}")))?;
        if n == 0 {
            return Err(Error::protocol("EOF before READY"));
        }
        while line.last() == Some(&b'\n') || line.last() == Some(&b'\r') {
            line.pop();
        }
        if line.as_slice() == READY_SENTINEL || line.starts_with(READY_SENTINEL) {
            return Ok(());
        }
    }
}

fn dispatch_loop(mut reader: BufReader<Box<dyn Read + Send>>, shared: Arc<Shared>) {
    let fail = |shared: &Shared, err: String| {
        shared.telemetry.lock().dispatcher_error = Some(err.clone());
        let pending: Vec<_> = shared.pending.lock().drain().map(|(_, tx)| tx).collect();
        for tx in pending {
            let _ = tx.send(ServeEvent::Error(err.clone()));
        }
    };

    loop {
        let mut line = Vec::new();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) => {
                if !*shared.closed.lock() {
                    fail(&shared, "colibri engine exited unexpectedly".into());
                }
                break;
            }
            Ok(_) => {}
            Err(e) => {
                fail(&shared, format!("engine read error: {e}"));
                break;
            }
        }
        while line.last() == Some(&b'\n') || line.last() == Some(&b'\r') {
            line.pop();
        }
        let text = String::from_utf8_lossy(&line);
        let fields: Vec<&str> = text.split_whitespace().collect();
        if fields.is_empty() {
            continue;
        }
        let kind = fields[0];
        match kind {
            "DATA" if fields.len() == 3 => {
                let request_id = fields[1].to_string();
                let size: usize = match fields[2].parse() {
                    Ok(s) if s <= 65536 => s,
                    _ => {
                        fail(&shared, "invalid engine DATA size".into());
                        break;
                    }
                };
                let mut data = vec![0u8; size];
                if let Err(e) = reader.read_exact(&mut data) {
                    fail(&shared, format!("truncated engine DATA payload: {e}"));
                    break;
                }
                let mut nl = [0u8; 1];
                if reader.read_exact(&mut nl).is_err() || nl != *b"\n" {
                    fail(&shared, "invalid engine DATA terminator".into());
                    break;
                }
                if let Some(tx) = shared.pending.lock().get(&request_id) {
                    let _ = tx.send(ServeEvent::Data(data));
                }
            }
            "ACCEPT" if fields.len() >= 3 => {
                let request_id = fields[1].to_string();
                let prompt_tokens: u64 = fields[2].parse().unwrap_or(0);
                if let Some(tx) = shared.pending.lock().get(&request_id) {
                    let _ = tx.send(ServeEvent::Accept { prompt_tokens });
                }
            }
            "DONE" if fields.len() >= 7 => {
                let request_id = fields[1].to_string();
                match parse_stats(&fields[2..]) {
                    Ok(stats) => {
                        if let Some(tx) = shared.pending.lock().remove(&request_id) {
                            let _ = tx.send(ServeEvent::Done(stats));
                        }
                    }
                    Err(e) => {
                        if let Some(tx) = shared.pending.lock().remove(&request_id) {
                            let _ = tx.send(ServeEvent::Error(e));
                        }
                    }
                }
            }
            "HWINFO" if fields.len() >= 7 => {
                let parts = fields[6..].join(" ");
                let (cpu, gpu) = match parts.split_once('|') {
                    Some((c, g)) => (c.trim().to_string(), g.trim().to_string()),
                    None => (parts, String::new()),
                };
                let snap = HwinfoSnap {
                    cores: fields[1].parse().unwrap_or(0),
                    ram_total_gb: fields[2].parse().unwrap_or(0.0),
                    ram_avail_gb: fields[3].parse().unwrap_or(0.0),
                    gpus: fields[4].parse().unwrap_or(0),
                    vram_total_gb: fields[5].parse().unwrap_or(0.0),
                    cpu,
                    gpu,
                };
                shared.telemetry.lock().hwinfo = Some(snap);
            }
            "EMAP" if fields.len() == 4 => {
                let rows: u32 = fields[1].parse().unwrap_or(0);
                let cols: u32 = fields[2].parse().unwrap_or(0);
                shared.telemetry.lock().emap_hex = Some((rows, cols, fields[3].to_string()));
            }
            "HITS" if fields.len() == 4 => {
                let mut t = shared.telemetry.lock();
                t.hits_hex = Some(fields[3].to_string());
                t.hits_seq += 1;
            }
            "PROF" if fields.len() >= 10 => {
                let turn = ProfileTurn {
                    wall_s: fields[1].parse().unwrap_or(0.0),
                    prompt_tokens: fields[2].parse().unwrap_or(0),
                    completion_tokens: fields[3].parse().unwrap_or(0),
                    expert_disk_s: fields[4].parse().unwrap_or(0.0),
                    expert_wait_s: fields[5].parse().unwrap_or(0.0),
                    expert_matmul_s: fields[6].parse().unwrap_or(0.0),
                    attention_s: fields[7].parse().unwrap_or(0.0),
                    lm_head_s: fields[8].parse().unwrap_or(0.0),
                    forwards: fields[9].parse().unwrap_or(0),
                };
                let mut t = shared.telemetry.lock();
                t.profile.push(turn);
                if t.profile.len() > 120 {
                    let drain = t.profile.len() - 120;
                    t.profile.drain(0..drain);
                }
                t.profile_seq += 1;
            }
            "TIERS" if fields.len() >= 6 => {
                let snap = TiersSnap {
                    vram: fields[1].parse().unwrap_or(0),
                    ram: fields[2].parse().unwrap_or(0),
                    disk: fields[3].parse().unwrap_or(0),
                    vram_gb: fields[4].parse().unwrap_or(0.0),
                    ram_gb: fields[5].parse().unwrap_or(0.0),
                };
                shared.telemetry.lock().tiers = Some(snap);
            }
            "ERROR" if fields.len() >= 2 => {
                let request_id = fields[1].to_string();
                let message = if fields.len() > 2 {
                    fields[2..].join(" ")
                } else {
                    "engine request failed".into()
                };
                if let Some(tx) = shared.pending.lock().remove(&request_id) {
                    let _ = tx.send(ServeEvent::Error(message));
                }
            }
            "STAT" => {}
            other => {
                tracing::debug!(kind = other, "ignoring unknown engine line");
            }
        }
    }
}

fn parse_stats(fields: &[&str]) -> std::result::Result<DoneStats, String> {
    if fields.len() < 5 || fields[0] != "STAT" {
        return Err(format!("invalid engine status: {}", fields.join(" ")));
    }
    Ok(DoneStats {
        completion_tokens: fields[1].parse().unwrap_or(0),
        tokens_per_second: fields[2].parse().unwrap_or(0.0),
        cache_hit_percent: fields[3].parse().unwrap_or(0.0),
        rss_gb: fields[4].parse().unwrap_or(0.0),
        prompt_tokens: fields.get(5).and_then(|s| s.parse().ok()).unwrap_or(0),
        length_limited: fields
            .get(6)
            .and_then(|s| s.parse::<u32>().ok())
            .map(|v| v != 0)
            .unwrap_or(false),
    })
}

// EMAP pack/unpack live in `crate::visual` (shared by process mux + FFI poll).
pub use crate::visual::{pack_expert_cell, unpack_expert_cell};

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn emap_cell_roundtrip() {
        let b = pack_expert_cell(2, 15);
        assert_eq!(unpack_expert_cell(b), (2, 15));
    }

    #[test]
    fn mock_generate_submit_done() {
        let (host_w, eng_r) = chan_pipe();
        let (eng_w, host_r) = chan_pipe();

        let eng = thread::spawn(move || {
            let mut eng_w = eng_w;
            let mut eng_r = BufReader::new(eng_r);
            eng_w.write_all(b"\x01\x01READY\x01\x01\n").unwrap();
            eng_w
                .write_all(b"HWINFO 8 32.0 16.0 0 0.0 cpu|gpu\n")
                .unwrap();
            eng_w.write_all(b"TIERS 0 10 90 0.0 1.5\n").unwrap();
            eng_w.flush().unwrap();
            let mut line = String::new();
            eng_r.read_line(&mut line).unwrap();
            assert!(line.starts_with("SUBMIT "));
            let parts: Vec<&str> = line.split_whitespace().collect();
            let id = parts[1];
            let nbytes: usize = parts[3].parse().unwrap();
            let mut payload = vec![0u8; nbytes];
            eng_r.read_exact(&mut payload).unwrap();
            let mut nl = [0u8; 1];
            eng_r.read_exact(&mut nl).unwrap();
            let tok = b"hi";
            writeln!(eng_w, "DATA {id} {}", tok.len()).unwrap();
            eng_w.write_all(tok).unwrap();
            eng_w.write_all(b"\n").unwrap();
            writeln!(eng_w, "DONE {id} STAT 1 10.0 50.0 1.2 3 0").unwrap();
            eng_w.flush().unwrap();
        });

        let mut client = ServeClient::from_pipes(None, Box::new(host_w), Box::new(host_r)).unwrap();
        // Allow dispatcher to process startup telemetry.
        thread::sleep(std::time::Duration::from_millis(50));
        assert!(client.hwinfo().is_some());
        assert!(client.tiers().is_some());

        let result = client
            .generate(&GenerateRequest {
                prompt: "hello".into(),
                max_tokens: 8,
                temperature: 0.7,
                top_p: 0.9,
                cache_slot: 0,
                grammar: None,
                request_id: None,
            })
            .unwrap();
        assert_eq!(result.text, "hi");
        assert_eq!(result.stats.completion_tokens, 1);
        assert!((result.stats.tokens_per_second - 10.0).abs() < 1e-6);
        eng.join().unwrap();
        let _ = client.shutdown();
    }

    /// Cancel must write the CANCEL line (distinct from STOP).
    #[test]
    fn cancel_request_writes_cancel_line() {
        let (host_w, eng_r) = chan_pipe();
        let (eng_w, host_r) = chan_pipe();

        let eng = thread::spawn(move || {
            let mut eng_w = eng_w;
            let mut eng_r = BufReader::new(eng_r);
            eng_w.write_all(b"\x01\x01READY\x01\x01\n").unwrap();
            eng_w.flush().unwrap();
            let mut line = String::new();
            eng_r.read_line(&mut line).unwrap();
            assert_eq!(line, "CANCEL 99\n", "expected CANCEL wire, got {line:?}");
        });

        let mut client = ServeClient::from_pipes(None, Box::new(host_w), Box::new(host_r)).unwrap();
        client.cancel_request(99).unwrap();
        eng.join().unwrap();
        let _ = client.shutdown();
    }

    /// Explicit request_id is the mux SUBMIT id on the wire.
    #[test]
    fn begin_generate_uses_explicit_request_id_on_submit() {
        let (host_w, eng_r) = chan_pipe();
        let (eng_w, host_r) = chan_pipe();

        let eng = thread::spawn(move || {
            let mut eng_w = eng_w;
            let mut eng_r = BufReader::new(eng_r);
            eng_w.write_all(b"\x01\x01READY\x01\x01\n").unwrap();
            eng_w.flush().unwrap();
            let mut line = String::new();
            eng_r.read_line(&mut line).unwrap();
            assert!(
                line.starts_with("SUBMIT 42 "),
                "expected SUBMIT 42, got {line:?}"
            );
            let parts: Vec<&str> = line.split_whitespace().collect();
            let nbytes: usize = parts[3].parse().unwrap();
            let mut payload = vec![0u8; nbytes];
            eng_r.read_exact(&mut payload).unwrap();
            let mut nl = [0u8; 1];
            eng_r.read_exact(&mut nl).unwrap();
            writeln!(eng_w, "DONE 42 STAT 0 0.0 0.0 0.0 0 0").unwrap();
            eng_w.flush().unwrap();
        });

        let mut client = ServeClient::from_pipes(None, Box::new(host_w), Box::new(host_r)).unwrap();
        let flight = client
            .begin_generate(&GenerateRequest {
                prompt: "x".into(),
                max_tokens: 4,
                request_id: Some(42),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(flight.request_id(), 42);
        let result = flight.recv_loop(|_| Ok(())).unwrap();
        assert_eq!(result.text, "");
        eng.join().unwrap();
        let _ = client.shutdown();
    }

    #[test]
    fn stop_request_writes_stop_line() {
        let (host_w, eng_r) = chan_pipe();
        let (eng_w, host_r) = chan_pipe();

        let eng = thread::spawn(move || {
            let mut eng_w = eng_w;
            let mut eng_r = BufReader::new(eng_r);
            eng_w.write_all(b"\x01\x01READY\x01\x01\n").unwrap();
            eng_w.flush().unwrap();
            let mut line = String::new();
            eng_r.read_line(&mut line).unwrap();
            assert_eq!(line, "STOP 7\n", "expected STOP wire, got {line:?}");
        });

        let mut client = ServeClient::from_pipes(None, Box::new(host_w), Box::new(host_r)).unwrap();
        client.stop_request(7).unwrap();
        eng.join().unwrap();
        let _ = client.shutdown();
    }

    #[test]
    fn explicit_request_id_zero_is_rejected() {
        let (host_w, _eng_r) = chan_pipe();
        let (eng_w, host_r) = chan_pipe();
        let eng = thread::spawn(move || {
            let mut eng_w = eng_w;
            eng_w.write_all(b"\x01\x01READY\x01\x01\n").unwrap();
            eng_w.flush().unwrap();
            thread::sleep(std::time::Duration::from_millis(100));
        });
        let client = ServeClient::from_pipes(None, Box::new(host_w), Box::new(host_r)).unwrap();
        let err = client
            .begin_generate(&GenerateRequest {
                prompt: "x".into(),
                request_id: Some(0),
                ..Default::default()
            })
            .unwrap_err();
        assert!(
            err.to_string().contains("non-zero"),
            "unexpected error: {err}"
        );
        drop(client);
        let _ = eng.join();
    }

    /// Explicit id advances the auto allocator past that id.
    #[test]
    fn explicit_request_id_bumps_next_auto_id() {
        let (host_w, eng_r) = chan_pipe();
        let (eng_w, host_r) = chan_pipe();

        let eng = thread::spawn(move || {
            let mut eng_w = eng_w;
            let mut eng_r = BufReader::new(eng_r);
            eng_w.write_all(b"\x01\x01READY\x01\x01\n").unwrap();
            eng_w.flush().unwrap();

            for expected_id in ["42", "43"] {
                let mut line = String::new();
                eng_r.read_line(&mut line).unwrap();
                assert!(
                    line.starts_with(&format!("SUBMIT {expected_id} ")),
                    "expected SUBMIT {expected_id}, got {line:?}"
                );
                let parts: Vec<&str> = line.split_whitespace().collect();
                let nbytes: usize = parts[3].parse().unwrap();
                let mut payload = vec![0u8; nbytes];
                eng_r.read_exact(&mut payload).unwrap();
                let mut nl = [0u8; 1];
                eng_r.read_exact(&mut nl).unwrap();
                writeln!(eng_w, "DONE {expected_id} STAT 0 0.0 0.0 0.0 0 0").unwrap();
                eng_w.flush().unwrap();
            }
        });

        let mut client = ServeClient::from_pipes(None, Box::new(host_w), Box::new(host_r)).unwrap();
        let flight42 = client
            .begin_generate(&GenerateRequest {
                prompt: "a".into(),
                max_tokens: 4,
                request_id: Some(42),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(flight42.request_id(), 42);
        flight42.recv_loop(|_| Ok(())).unwrap();

        let flight_auto = client
            .begin_generate(&GenerateRequest {
                prompt: "b".into(),
                max_tokens: 4,
                request_id: None,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            flight_auto.request_id(),
            43,
            "auto id must continue past explicit 42"
        );
        flight_auto.recv_loop(|_| Ok(())).unwrap();
        eng.join().unwrap();
        let _ = client.shutdown();
    }

    /// Duplicate in-flight explicit id is rejected; first flight stays registered.
    #[test]
    fn duplicate_in_flight_request_id_is_rejected() {
        let (host_w, eng_r) = chan_pipe();
        let (eng_w, host_r) = chan_pipe();

        let eng = thread::spawn(move || {
            let mut eng_w = eng_w;
            let mut eng_r = BufReader::new(eng_r);
            eng_w.write_all(b"\x01\x01READY\x01\x01\n").unwrap();
            eng_w.flush().unwrap();
            let mut line = String::new();
            eng_r.read_line(&mut line).unwrap();
            assert!(line.starts_with("SUBMIT 7 "), "got {line:?}");
            let parts: Vec<&str> = line.split_whitespace().collect();
            let nbytes: usize = parts[3].parse().unwrap();
            let mut payload = vec![0u8; nbytes];
            eng_r.read_exact(&mut payload).unwrap();
            let mut nl = [0u8; 1];
            eng_r.read_exact(&mut nl).unwrap();
            // Hold the first flight open until host finishes the duplicate check.
            thread::sleep(std::time::Duration::from_millis(80));
            writeln!(eng_w, "DONE 7 STAT 0 0.0 0.0 0.0 0 0").unwrap();
            eng_w.flush().unwrap();
        });

        let mut client = ServeClient::from_pipes(None, Box::new(host_w), Box::new(host_r)).unwrap();
        let first = client
            .begin_generate(&GenerateRequest {
                prompt: "first".into(),
                max_tokens: 4,
                request_id: Some(7),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(first.request_id(), 7);

        let dup_err = client
            .begin_generate(&GenerateRequest {
                prompt: "second".into(),
                max_tokens: 4,
                request_id: Some(7),
                ..Default::default()
            })
            .unwrap_err();
        assert!(
            dup_err.to_string().contains("already in flight"),
            "unexpected error: {dup_err}"
        );

        // First flight still completes (was not overwritten).
        first.recv_loop(|_| Ok(())).unwrap();
        eng.join().unwrap();
        let _ = client.shutdown();
    }

    /// Stdin write failure after pending insert must remove the id from pending.
    #[test]
    fn begin_generate_write_failure_cleans_pending() {
        /// Writer that accepts READY handshake setup then fails every write.
        struct FailWriter;

        impl Write for FailWriter {
            fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "simulated stdin failure",
                ))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "simulated stdin failure",
                ))
            }
        }

        let (eng_w, host_r) = chan_pipe();
        let eng = thread::spawn(move || {
            let mut eng_w = eng_w;
            eng_w.write_all(b"\x01\x01READY\x01\x01\n").unwrap();
            eng_w.flush().unwrap();
            thread::sleep(std::time::Duration::from_millis(50));
        });

        let client = ServeClient::from_pipes(None, Box::new(FailWriter), Box::new(host_r)).unwrap();
        let err = client
            .begin_generate(&GenerateRequest {
                prompt: "x".into(),
                max_tokens: 4,
                request_id: Some(11),
                ..Default::default()
            })
            .unwrap_err();
        assert!(
            err.to_string().contains("simulated stdin failure")
                || err.to_string().contains("BrokenPipe")
                || err.to_string().contains("broken pipe"),
            "unexpected error: {err}"
        );

        // Same explicit id must be usable again (not stuck in pending).
        // Use a real writer for the second attempt after swapping is not possible;
        // assert via a second begin_generate with FailWriter still failing write
        // but not "already in flight".
        let err2 = client
            .begin_generate(&GenerateRequest {
                prompt: "y".into(),
                max_tokens: 4,
                request_id: Some(11),
                ..Default::default()
            })
            .unwrap_err();
        assert!(
            !err2.to_string().contains("already in flight"),
            "pending leaked after write failure: {err2}"
        );
        assert!(
            err2.to_string().contains("simulated stdin failure")
                || err2.to_string().contains("BrokenPipe")
                || err2.to_string().contains("broken pipe"),
            "unexpected second error: {err2}"
        );
        drop(client);
        let _ = eng.join();
    }

    /// Shutdown wakes blocked recv_loop (no hang when closed mid-generate).
    #[test]
    fn shutdown_wakes_pending_recv() {
        let (host_w, eng_r) = chan_pipe();
        let (eng_w, host_r) = chan_pipe();

        let eng = thread::spawn(move || {
            let mut eng_w = eng_w;
            let mut eng_r = BufReader::new(eng_r);
            eng_w.write_all(b"\x01\x01READY\x01\x01\n").unwrap();
            eng_w.flush().unwrap();
            let mut line = String::new();
            eng_r.read_line(&mut line).unwrap();
            assert!(line.starts_with("SUBMIT 3 "), "got {line:?}");
            let parts: Vec<&str> = line.split_whitespace().collect();
            let nbytes: usize = parts[3].parse().unwrap();
            let mut payload = vec![0u8; nbytes];
            eng_r.read_exact(&mut payload).unwrap();
            let mut nl = [0u8; 1];
            eng_r.read_exact(&mut nl).unwrap();
            writeln!(eng_w, "ACCEPT 3 1").unwrap();
            let tok = b"hi";
            writeln!(eng_w, "DATA 3 {}", tok.len()).unwrap();
            eng_w.write_all(tok).unwrap();
            eng_w.write_all(b"\n").unwrap();
            eng_w.flush().unwrap();
            // Never send DONE; host will shutdown and must still wake.
            thread::sleep(std::time::Duration::from_millis(500));
        });

        let mut client = ServeClient::from_pipes(None, Box::new(host_w), Box::new(host_r)).unwrap();
        let flight = client
            .begin_generate(&GenerateRequest {
                prompt: "p".into(),
                max_tokens: 8,
                request_id: Some(3),
                ..Default::default()
            })
            .unwrap();

        let (done_tx, done_rx) = mpsc::channel();
        let recv_thread = thread::spawn(move || {
            let r = flight.recv_loop(|_| Ok(()));
            let _ = done_tx.send(());
            r
        });

        // Wait until DATA has had time to arrive, then shutdown.
        thread::sleep(std::time::Duration::from_millis(50));
        client.shutdown().unwrap();

        let finished = done_rx.recv_timeout(std::time::Duration::from_secs(2));
        assert!(
            finished.is_ok(),
            "recv_loop hung after shutdown (pending not drained)"
        );
        let err = recv_thread.join().unwrap().unwrap_err();
        assert!(
            err.to_string().contains("shutting down"),
            "unexpected error: {err}"
        );
        let _ = eng.join();
    }
}
