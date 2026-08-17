//! Engine process embed: locate binary, spawn with SERVE mux, generate API.
//!
//! C engines (`colibri`, `inkling`, `kimi_k3`, `deepseek_v4`) remain
//! **subprocesses** by default. Optional `feature = "ffi"` can link DeepSeek V4
//! CPU static code (`crate::ffi`); hosts must still pass the kill-switch
//! ([`crate::config::ColibriConfig::prefer_process`] default true,
//! `COLIBRI_FORCE_PROCESS=1`). This module always implements the **process**
//! path; FFI open lives under `crate::ffi`.
//!
//! Serve spawn applies [`crate::process_priority::apply_low_compute_priority`] so
//! the engine child runs at elevated Unix niceness (or Windows below-normal).
//! In-process FFI start/generate workers call
//! [`crate::process_priority::set_current_thread_nice`] and the OpenMP team
//! hook; the GPUI thread is not demoted.
//!
//! Port of spawn/env wiring from `c/openai_server.py` (`Engine.__init__`) and
//! binary selection from `c/coli` (`engine_for`, `COLI_ENGINE`).

pub mod locate;
pub mod serve;

#[cfg(all(feature = "runtime", feature = "stream"))]
pub mod duplex;

pub use crate::visual::{HwinfoSnap, TiersSnap, decode_hex_bytes};
pub use crate::visual::{pack_expert_cell, unpack_expert_cell};
pub use locate::{
    EngineLocate, default_engine_candidates, engine_override_from_env, locate_engine,
};
pub use serve::{
    DoneStats, GenerateRequest, GenerateResult, InFlightGenerate, READY_SENTINEL, ServeClient,
    ServeEvent,
};

#[cfg(all(feature = "runtime", feature = "stream"))]
pub use duplex::EngineDuplex;

use std::path::Path;
use std::sync::Arc;

use parking_lot::Mutex;

use crate::config::ColibriConfig;
use crate::error::{Error, Result};
use crate::model::{ModelFamily, model_arch};
use crate::plan::PlacementPlan;
use crate::visual::{ExpertHits, ExpertMap, ProfileTurn, VisualSnapshot};

/// High-level handle to a supervised engine process.
///
/// Cheap to [`Clone`]: clones share the same process and mux client. A clone
/// may call [`EngineHandle::with_client`] (STOP/CANCEL) while another thread is
/// inside [`EngineHandle::generate_stream`].
#[derive(Clone)]
pub struct EngineHandle {
    inner: Arc<Mutex<EngineInner>>,
}

struct EngineInner {
    client: ServeClient,
    config: ColibriConfig,
    family: ModelFamily,
    visual: VisualSnapshot,
}

impl EngineHandle {
    /// Whether this config/env must use the process serve path (kill-switch).
    ///
    /// See [`ColibriConfig::must_use_process`]. Always true for this handle's
    /// start methods (they only spawn processes); use the helper when choosing
    /// between process and experimental [`crate::ffi`] open.
    pub fn must_use_process(config: &ColibriConfig) -> bool {
        config.must_use_process()
    }

    /// Start an engine from config (must set model path; resolves engine binary).
    ///
    /// Always spawns a **subprocess** (serve mux). Does not use in-process FFI
    /// even when `feature = "ffi"` is linked; set `prefer_process = false` and
    /// call `crate::ffi` APIs only when the host explicitly wants V4 embed.
    #[cfg(feature = "runtime")]
    pub fn start_blocking(config: ColibriConfig) -> Result<Self> {
        // Kill-switch is informational for process start: we always process-spawn.
        // Hosts that prefer FFI must branch before calling this (see prefer_ffi_path).
        let _ = Self::must_use_process(&config);
        let model = config
            .model
            .as_ref()
            .ok_or_else(|| Error::invalid("model path required"))?;
        let family = model_arch(model);
        let engine_path = if let Some(ref e) = config.engine {
            e.clone()
        } else {
            locate_engine(EngineLocate {
                family,
                override_path: engine_override_from_env(),
                search_roots: vec![],
            })?
        };
        let env = config.serve_env()?;
        Self::start_with_env(config, family, &engine_path, env)
    }

    /// Start with an already-built placement plan applied into the env.
    #[cfg(feature = "runtime")]
    pub fn start_with_plan(config: ColibriConfig, plan: &PlacementPlan) -> Result<Self> {
        let model = config
            .model
            .as_ref()
            .ok_or_else(|| Error::invalid("model path required"))?;
        let family = model_arch(model);
        let engine_path = if let Some(ref e) = config.engine {
            e.clone()
        } else {
            locate_engine(EngineLocate {
                family,
                override_path: engine_override_from_env(),
                search_roots: vec![],
            })?
        };
        let env = config.serve_env_with_plan(plan)?;
        Self::start_with_env(config, family, &engine_path, env)
    }

    #[cfg(feature = "runtime")]
    fn start_with_env(
        config: ColibriConfig,
        family: ModelFamily,
        engine_path: &Path,
        env: crate::config::EnvMap,
    ) -> Result<Self> {
        let cap = config.cap;
        let client = ServeClient::spawn(engine_path, &env, cap, family)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(EngineInner {
                client,
                config,
                family,
                visual: VisualSnapshot::default(),
            })),
        })
    }

    /// Wrap an already-connected mock/serve client (tests).
    pub fn from_client(client: ServeClient, config: ColibriConfig, family: ModelFamily) -> Self {
        Self {
            inner: Arc::new(Mutex::new(EngineInner {
                client,
                config,
                family,
                visual: VisualSnapshot::default(),
            })),
        }
    }

    /// Run one generation request (blocking full result).
    #[cfg(feature = "runtime")]
    pub fn generate(&self, req: GenerateRequest) -> Result<GenerateResult> {
        self.generate_stream(req, |_| Ok(()))
    }

    /// Run one generation with progressive mux events (DATA tokens as they arrive).
    ///
    /// The inner mutex is held only while writing SUBMIT and while absorbing
    /// visual telemetry after DONE. It is **not** held across the receive loop,
    /// so another thread may call [`Self::with_client`] to send STOP or CANCEL
    /// for the in-flight mux id.
    #[cfg(feature = "runtime")]
    pub fn generate_stream<F>(
        &self,
        req: GenerateRequest,
        mut on_event: F,
    ) -> Result<GenerateResult>
    where
        F: FnMut(&ServeEvent) -> Result<()>,
    {
        let flight = {
            let g = self.inner.lock();
            g.client.begin_generate(&req)?
        };
        // Lock released: concurrent stop_request / cancel_request can proceed.
        let result = flight.recv_loop(&mut on_event)?;
        {
            let mut g = self.inner.lock();
            let mut snap = VisualSnapshot::default();
            snap.absorb_from_client(&g.client);
            g.visual = snap;
        }
        Ok(result)
    }

    /// Access the underlying serve client (stop, cancel, telemetry refresh).
    ///
    /// Safe to call from another thread while [`Self::generate_stream`] is waiting
    /// on mux events (the generate path does not hold the handle mutex during recv).
    #[cfg(feature = "runtime")]
    pub fn with_client<R>(&self, f: impl FnOnce(&ServeClient) -> R) -> R {
        f(&self.inner.lock().client)
    }

    /// Refresh visual snapshot from latest mux telemetry without generating.
    #[cfg(feature = "runtime")]
    pub fn pump_visual(&self) {
        let mut g = self.inner.lock();
        let mut snap = g.visual.clone();
        snap.absorb_from_client(&g.client);
        g.visual = snap;
    }

    /// Latest visual snapshot (updated after generate / pump).
    pub fn visual_snapshot(&self) -> VisualSnapshot {
        self.inner.lock().visual.clone()
    }

    pub fn tiers(&self) -> Option<TiersSnap> {
        self.inner.lock().client.tiers()
    }

    pub fn hwinfo(&self) -> Option<HwinfoSnap> {
        self.inner.lock().client.hwinfo()
    }

    pub fn expert_map(&self) -> Option<ExpertMap> {
        self.inner.lock().visual.expert_map.clone()
    }

    pub fn expert_hits(&self) -> Option<ExpertHits> {
        self.inner.lock().visual.expert_hits.clone()
    }

    pub fn profile_window(&self) -> Vec<ProfileTurn> {
        self.inner.lock().visual.profile.clone()
    }

    pub fn family(&self) -> ModelFamily {
        self.inner.lock().family
    }

    pub fn config(&self) -> ColibriConfig {
        self.inner.lock().config.clone()
    }

    /// Graceful stop (drop of the last clone also stops).
    pub fn stop(&self) -> Result<()> {
        self.inner.lock().client.shutdown()
    }
}

impl Drop for EngineInner {
    fn drop(&mut self) {
        // Runs when the last EngineHandle clone releases the Arc.
        let _ = self.client.shutdown();
    }
}

#[cfg(all(test, feature = "runtime"))]
mod tests {
    use super::*;
    use crate::engine::serve::{GenerateRequest, READY_SENTINEL, ServeClient};
    use std::io::{BufRead, BufReader, Read, Write};
    use std::sync::Arc as StdArc;
    use std::sync::atomic::{AtomicBool, Ordering};
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

    /// Mid-stream Stop from another thread must complete without deadlock.
    ///
    /// Old bug: `generate_stream` held `EngineHandle` mutex for the whole recv
    /// loop, so `with_client(stop_request)` could never acquire the lock.
    #[test]
    fn mid_stream_stop_no_deadlock() {
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
            assert!(line.starts_with("SUBMIT "), "got {line}");
            let parts: Vec<&str> = line.split_whitespace().collect();
            let id = parts[1].to_string();
            let nbytes: usize = parts[3].parse().unwrap();
            let mut payload = vec![0u8; nbytes];
            eng_r.read_exact(&mut payload).unwrap();
            let mut nl = [0u8; 1];
            eng_r.read_exact(&mut nl).unwrap();

            writeln!(eng_w, "ACCEPT {id} 1").unwrap();
            let tok = b"hi";
            writeln!(eng_w, "DATA {id} {}", tok.len()).unwrap();
            eng_w.write_all(tok).unwrap();
            eng_w.write_all(b"\n").unwrap();
            eng_w.flush().unwrap();

            // Wait for STOP from host (concurrent with generate_stream recv).
            line.clear();
            eng_r.read_line(&mut line).unwrap();
            assert_eq!(line, format!("STOP {id}\n"), "expected STOP, got {line:?}");

            writeln!(eng_w, "DONE {id} STAT 1 10.0 0.0 0.5 1 0").unwrap();
            eng_w.flush().unwrap();
        });

        let client = ServeClient::from_pipes(None, Box::new(host_w), Box::new(host_r)).unwrap();
        let handle = EngineHandle::from_client(client, ColibriConfig::default(), ModelFamily::Glm);
        let handle_stop = handle.clone();

        let saw_data = StdArc::new(AtomicBool::new(false));
        let saw_data_gen = StdArc::clone(&saw_data);

        let gen_thread = thread::spawn(move || {
            handle.generate_stream(
                GenerateRequest {
                    prompt: "p".into(),
                    max_tokens: 64,
                    request_id: Some(5),
                    ..Default::default()
                },
                |ev| {
                    if matches!(ev, ServeEvent::Data(_)) {
                        saw_data_gen.store(true, Ordering::SeqCst);
                    }
                    Ok(())
                },
            )
        });

        // Wait until first DATA is delivered (generate is in recv loop).
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !saw_data.load(Ordering::SeqCst) {
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for DATA before stop"
            );
            thread::sleep(Duration::from_millis(5));
        }

        // Concurrent stop while generate holds no outer lock.
        handle_stop
            .with_client(|c| c.stop_request(5))
            .expect("stop_request");

        let result = gen_thread
            .join()
            .expect("generate thread panicked")
            .expect("generate_stream failed");
        assert_eq!(result.text, "hi");
        eng.join().unwrap();
    }

    /// Mid-stream Cancel from another thread must complete without deadlock
    /// and surface ERROR CANCELLED (not a successful Done).
    #[test]
    fn mid_stream_cancel_no_deadlock() {
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
            assert!(line.starts_with("SUBMIT "), "got {line}");
            let parts: Vec<&str> = line.split_whitespace().collect();
            let id = parts[1].to_string();
            let nbytes: usize = parts[3].parse().unwrap();
            let mut payload = vec![0u8; nbytes];
            eng_r.read_exact(&mut payload).unwrap();
            let mut nl = [0u8; 1];
            eng_r.read_exact(&mut nl).unwrap();

            writeln!(eng_w, "ACCEPT {id} 1").unwrap();
            let tok = b"hi";
            writeln!(eng_w, "DATA {id} {}", tok.len()).unwrap();
            eng_w.write_all(tok).unwrap();
            eng_w.write_all(b"\n").unwrap();
            eng_w.flush().unwrap();

            line.clear();
            eng_r.read_line(&mut line).unwrap();
            assert_eq!(
                line,
                format!("CANCEL {id}\n"),
                "expected CANCEL, got {line:?}"
            );

            writeln!(eng_w, "ERROR {id} CANCELLED").unwrap();
            eng_w.flush().unwrap();
        });

        let client = ServeClient::from_pipes(None, Box::new(host_w), Box::new(host_r)).unwrap();
        let handle = EngineHandle::from_client(client, ColibriConfig::default(), ModelFamily::Glm);
        let handle_cancel = handle.clone();

        let saw_data = StdArc::new(AtomicBool::new(false));
        let saw_data_gen = StdArc::clone(&saw_data);

        let gen_thread = thread::spawn(move || {
            handle.generate_stream(
                GenerateRequest {
                    prompt: "p".into(),
                    max_tokens: 64,
                    request_id: Some(5),
                    ..Default::default()
                },
                |ev| {
                    if matches!(ev, ServeEvent::Data(_)) {
                        saw_data_gen.store(true, Ordering::SeqCst);
                    }
                    Ok(())
                },
            )
        });

        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !saw_data.load(Ordering::SeqCst) {
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for DATA before cancel"
            );
            thread::sleep(Duration::from_millis(5));
        }

        handle_cancel
            .with_client(|c| c.cancel_request(5))
            .expect("cancel_request");

        let err = gen_thread
            .join()
            .expect("generate thread panicked")
            .expect_err("cancel should fail generate_stream, not Done");
        assert!(
            err.to_string().contains("CANCELLED"),
            "unexpected error: {err}"
        );
        eng.join().unwrap();
    }

    /// Process stop mid-generate must wake recv (no hang after unlock-during-recv).
    #[test]
    fn shutdown_during_generate_wakes_recv() {
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
            assert!(line.starts_with("SUBMIT "), "got {line}");
            let parts: Vec<&str> = line.split_whitespace().collect();
            let id = parts[1].to_string();
            let nbytes: usize = parts[3].parse().unwrap();
            let mut payload = vec![0u8; nbytes];
            eng_r.read_exact(&mut payload).unwrap();
            let mut nl = [0u8; 1];
            eng_r.read_exact(&mut nl).unwrap();

            writeln!(eng_w, "ACCEPT {id} 1").unwrap();
            let tok = b"hi";
            writeln!(eng_w, "DATA {id} {}", tok.len()).unwrap();
            eng_w.write_all(tok).unwrap();
            eng_w.write_all(b"\n").unwrap();
            eng_w.flush().unwrap();
            // No DONE: host calls EngineHandle::stop which must wake generate.
            thread::sleep(Duration::from_millis(500));
        });

        let client = ServeClient::from_pipes(None, Box::new(host_w), Box::new(host_r)).unwrap();
        let handle = EngineHandle::from_client(client, ColibriConfig::default(), ModelFamily::Glm);
        let handle_stop = handle.clone();

        let saw_data = StdArc::new(AtomicBool::new(false));
        let saw_data_gen = StdArc::clone(&saw_data);

        let (done_tx, done_rx) = mpsc::channel();
        let gen_thread = thread::spawn(move || {
            let r = handle.generate_stream(
                GenerateRequest {
                    prompt: "p".into(),
                    max_tokens: 64,
                    request_id: Some(9),
                    ..Default::default()
                },
                |ev| {
                    if matches!(ev, ServeEvent::Data(_)) {
                        saw_data_gen.store(true, Ordering::SeqCst);
                    }
                    Ok(())
                },
            );
            let _ = done_tx.send(());
            r
        });

        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !saw_data.load(Ordering::SeqCst) {
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for DATA before process stop"
            );
            thread::sleep(Duration::from_millis(5));
        }

        handle_stop.stop().expect("EngineHandle::stop");

        let finished = done_rx.recv_timeout(Duration::from_secs(2));
        assert!(
            finished.is_ok(),
            "generate_stream hung after EngineHandle::stop (pending not drained)"
        );
        let err = gen_thread
            .join()
            .expect("generate thread panicked")
            .expect_err("stop should fail generate_stream");
        assert!(
            err.to_string().contains("shutting down"),
            "unexpected error: {err}"
        );
        let _ = eng.join();
    }
}
