//! Safe wrappers around experimental `coli_v4_*` engine/session API.

use std::ffi::{CStr, CString};
use std::os::raw::{c_float, c_int, c_void};
use std::path::{Path, PathBuf};
use std::ptr;

use super::bindings::{
    self, ColiV4Engine, ColiV4EngineMemorySummary, ColiV4EngineOpenOptions, ColiV4Session,
    ColiV4SessionCreateOptions, ColiV4SessionGenerateOptions, ColiV4SessionGenerateStats,
};
use crate::config::force_process_from_env;
use crate::error::{Error, Result};
use crate::model::{ModelFamily, ModelInfo, ModelSizeInfo};

const ERROR_BUF: usize = 512;

/// Options for [`V4Engine::open`].
#[derive(Debug, Clone)]
pub struct V4EngineOpenOptions {
    pub model_dir: PathBuf,
    /// 0 = use OS available memory (engine default).
    pub memory_limit_bytes: u64,
    /// 0 = engine default (4096).
    pub context_tokens: i32,
    /// -1 = auto.
    pub pin_slots_per_layer: i32,
    /// 0 = auto.
    pub repin_interval: u64,
    pub no_dspark: bool,
}

impl V4EngineOpenOptions {
    pub fn new(model_dir: impl Into<PathBuf>) -> Self {
        Self {
            model_dir: model_dir.into(),
            memory_limit_bytes: 0,
            context_tokens: 0,
            pin_slots_per_layer: -1,
            repin_interval: 0,
            no_dspark: false,
        }
    }
}

/// Opaque in-process DeepSeek V4 engine (weights / maps).
///
/// Destroy all sessions before dropping the engine (enforced by session lifetime).
pub struct V4Engine {
    raw: *mut ColiV4Engine,
    size: ModelSizeInfo,
}

// Engine is not Send/Sync by default: C path uses process-global OpenMP / maps.
// Document single-owner use; leave !Send until a full thread model exists.
// (No unsafe impl Send.)

impl V4Engine {
    /// Open a V4 engine for `options.model_dir`.
    ///
    /// Fails if [`force_process_from_env`] is true (kill-switch) or the C open
    /// returns an error (missing weights, bad config, OOM, …).
    pub fn open(options: V4EngineOpenOptions) -> Result<Self> {
        if force_process_from_env() {
            return Err(Error::engine(
                "COLIBRI_FORCE_PROCESS is set; refusing in-process V4 open (use process serve path)",
            ));
        }
        super::apply_ffi_compute_niceness();
        let c_dir = path_to_cstring(&options.model_dir)?;

        let c_opts = ColiV4EngineOpenOptions {
            target_model_dir: c_dir.as_ptr(),
            memory_limit_bytes: options.memory_limit_bytes,
            context_tokens: options.context_tokens,
            pin_slots_per_layer: options.pin_slots_per_layer,
            repin_interval: options.repin_interval,
            no_dspark: if options.no_dspark { 1 } else { 0 },
        };

        let mut raw: *mut ColiV4Engine = ptr::null_mut();
        let mut err = [0i8; ERROR_BUF];
        // SAFETY: options pointers live for the call; error buffer is writable.
        let rc = unsafe {
            bindings::coli_v4_engine_open(&mut raw, &c_opts, err.as_mut_ptr(), err.len())
        };
        if rc != 0 || raw.is_null() {
            return Err(Error::engine(c_error_string(&err)));
        }
        let size = ModelInfo::inspect(&options.model_dir)
            .map(|i| i.size_info())
            .unwrap_or_else(|_| ModelSizeInfo {
                path: options.model_dir.clone(),
                family: Some(ModelFamily::DeepseekV4),
                engine_id: "deepseek_v4".into(),
                disk_bytes: 0,
                model_bytes: 0,
                dense_bytes: 0,
                expert_bytes: 0,
                param_count: None,
                shards: 0,
                tier_vram_bytes: None,
                tier_ram_bytes: None,
                tier_disk_bytes: None,
            });
        // Overlay C memory summary projected_bytes as tier_ram hint when known.
        let mut size = size;
        let mem = {
            let mut summary = ColiV4EngineMemorySummary::default();
            unsafe { bindings::coli_v4_engine_memory_summary(raw, &mut summary) };
            summary
        };
        if mem.projected_bytes > 0 {
            size.tier_ram_bytes = Some(mem.projected_bytes);
        }
        Ok(Self { raw, size })
    }

    /// Model size snapshot (`disk_bytes` from inspect when available).
    pub fn size_info(&self) -> ModelSizeInfo {
        self.size.clone()
    }

    pub fn model_dir(&self) -> Option<PathBuf> {
        // SAFETY: engine is live until drop.
        let p = unsafe { bindings::coli_v4_engine_target_model_dir(self.raw) };
        if p.is_null() {
            return None;
        }
        // SAFETY: C string owned by engine for its lifetime.
        let s = unsafe { CStr::from_ptr(p) };
        Some(PathBuf::from(s.to_string_lossy().as_ref()))
    }

    pub fn memory_summary(&self) -> ColiV4EngineMemorySummary {
        let mut summary = ColiV4EngineMemorySummary::default();
        // SAFETY: engine live; summary is stack POD.
        unsafe { bindings::coli_v4_engine_memory_summary(self.raw, &mut summary) };
        summary
    }

    /// Create a session that borrows this engine.
    pub fn create_session(&self, options: V4SessionCreateOptions) -> Result<V4Session<'_>> {
        let c_opts = ColiV4SessionCreateOptions {
            max_prompt_tokens: options.max_prompt_tokens,
            max_new_tokens_cap: options.max_new_tokens_cap,
        };
        let mut raw: *mut ColiV4Session = ptr::null_mut();
        let mut err = [0i8; ERROR_BUF];
        // SAFETY: engine outlives session via lifetime; options POD.
        let rc = unsafe {
            bindings::coli_v4_session_create(
                &mut raw,
                self.raw,
                &c_opts,
                err.as_mut_ptr(),
                err.len(),
            )
        };
        if rc != 0 || raw.is_null() {
            return Err(Error::engine(c_error_string(&err)));
        }
        Ok(V4Session {
            raw,
            _engine: std::marker::PhantomData,
        })
    }
}

impl Drop for V4Engine {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            // SAFETY: no sessions may outlive engine (lifetime on V4Session).
            unsafe { bindings::coli_v4_engine_destroy(self.raw) };
            self.raw = ptr::null_mut();
        }
    }
}

/// Session create options (maps to `ColiV4SessionCreateOptions`).
#[derive(Debug, Clone, Copy, Default)]
pub struct V4SessionCreateOptions {
    /// 0 = engine default (512).
    pub max_prompt_tokens: i32,
    /// 0 = engine default (512).
    pub max_new_tokens_cap: i32,
}

/// Per-generate options.
#[derive(Debug, Clone, Copy)]
pub struct V4GenerateOptions {
    pub max_new_tokens: i32,
    pub stop_at_sentence: bool,
    pub no_dspark: bool,
}

impl Default for V4GenerateOptions {
    fn default() -> Self {
        Self {
            max_new_tokens: 32,
            stop_at_sentence: false,
            no_dspark: false,
        }
    }
}

/// Stats from a completed generate call.
#[derive(Debug, Clone, Copy, Default)]
pub struct V4GenerateStats {
    pub prompt_tokens: i32,
    pub generated_tokens: i32,
    pub eos_stopped: bool,
    pub time_to_first_token_sec: f64,
    pub decode_sec: f64,
    pub speculative_drafted: u64,
    pub speculative_accepted: u64,
}

impl From<ColiV4SessionGenerateStats> for V4GenerateStats {
    fn from(s: ColiV4SessionGenerateStats) -> Self {
        Self {
            prompt_tokens: s.prompt_tokens,
            generated_tokens: s.generated_tokens,
            eos_stopped: s.eos_stopped != 0,
            time_to_first_token_sec: s.time_to_first_token_sec,
            decode_sec: s.decode_sec,
            speculative_drafted: s.speculative_drafted,
            speculative_accepted: s.speculative_accepted,
        }
    }
}

/// One token from the generate callback.
#[derive(Debug, Clone, Copy)]
pub struct V4TokenEvent {
    pub token: i32,
    pub logit: f32,
    pub position: i32,
    pub ordinal: i32,
}

/// Session borrowing an engine.
pub struct V4Session<'a> {
    raw: *mut ColiV4Session,
    _engine: std::marker::PhantomData<&'a V4Engine>,
}

impl V4Session<'_> {
    /// Run generation. `on_token` may return `Err` to stop cooperatively.
    pub fn generate<F>(
        &mut self,
        prompt: &str,
        options: V4GenerateOptions,
        mut on_token: F,
    ) -> Result<V4GenerateStats>
    where
        F: FnMut(V4TokenEvent) -> Result<()>,
    {
        if force_process_from_env() {
            return Err(Error::engine(
                "COLIBRI_FORCE_PROCESS is set; refusing in-process generate",
            ));
        }
        super::apply_ffi_compute_niceness();

        let c_opts = ColiV4SessionGenerateOptions {
            max_new_tokens: options.max_new_tokens,
            stop_at_sentence: if options.stop_at_sentence { 1 } else { 0 },
            no_dspark: if options.no_dspark { 1 } else { 0 },
        };

        // Callback bridge: C calls us; we call the closure.
        struct Ctx<'b, F> {
            f: &'b mut F,
            err: Option<Error>,
        }

        unsafe extern "C" fn trampoline<F>(
            user_data: *mut c_void,
            token: c_int,
            logit: c_float,
            position: c_int,
            ordinal: c_int,
        ) -> c_int
        where
            F: FnMut(V4TokenEvent) -> Result<()>,
        {
            // SAFETY: user_data is &mut Ctx set by generate for the call duration.
            let ctx = unsafe { &mut *(user_data as *mut Ctx<'_, F>) };
            match (ctx.f)(V4TokenEvent {
                token,
                logit,
                position,
                ordinal,
            }) {
                Ok(()) => 0,
                Err(e) => {
                    ctx.err = Some(e);
                    1 // non-zero stops generation
                }
            }
        }

        let mut ctx = Ctx {
            f: &mut on_token,
            err: None,
        };
        let mut stats = ColiV4SessionGenerateStats::default();
        let mut err = [0i8; ERROR_BUF];

        // SAFETY: session live; prompt bytes valid for call; trampoline matches F.
        let rc = unsafe {
            bindings::coli_v4_session_generate(
                self.raw,
                prompt.as_ptr() as *const _,
                prompt.len(),
                &c_opts,
                Some(trampoline::<F>),
                &mut ctx as *mut Ctx<'_, F> as *mut c_void,
                &mut stats,
                err.as_mut_ptr(),
                err.len(),
            )
        };

        if let Some(e) = ctx.err.take() {
            return Err(e);
        }
        if rc != 0 {
            return Err(Error::engine(c_error_string(&err)));
        }
        Ok(stats.into())
    }

    /// Accumulated generated text buffer from the last generate (UTF-8 best effort).
    pub fn generated_text(&self) -> Result<String> {
        let mut len: usize = 0;
        // First call: probe length.
        let rc = unsafe {
            bindings::coli_v4_session_generated_text(self.raw, ptr::null_mut(), 0, &mut len)
        };
        if rc != 0 {
            // Some builds write into buffer only; try a growing buffer.
            let mut buf = vec![0u8; 4096];
            let mut out_len = 0usize;
            let rc2 = unsafe {
                bindings::coli_v4_session_generated_text(
                    self.raw,
                    buf.as_mut_ptr() as *mut _,
                    buf.len(),
                    &mut out_len,
                )
            };
            if rc2 != 0 {
                return Err(Error::engine("coli_v4_session_generated_text failed"));
            }
            buf.truncate(out_len.min(buf.len()));
            return Ok(String::from_utf8_lossy(&buf).into_owned());
        }
        let mut buf = vec![0u8; len.saturating_add(1).max(1)];
        let mut out_len = 0usize;
        let rc = unsafe {
            bindings::coli_v4_session_generated_text(
                self.raw,
                buf.as_mut_ptr() as *mut _,
                buf.len(),
                &mut out_len,
            )
        };
        if rc != 0 {
            return Err(Error::engine("coli_v4_session_generated_text failed"));
        }
        buf.truncate(out_len.min(buf.len()));
        // Drop trailing NUL if present.
        if buf.last() == Some(&0) {
            buf.pop();
        }
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }
}

impl Drop for V4Session<'_> {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe { bindings::coli_v4_session_destroy(self.raw) };
            self.raw = ptr::null_mut();
        }
    }
}

fn c_error_string(err: &[i8]) -> String {
    let bytes: Vec<u8> = err
        .iter()
        .map(|&c| c as u8)
        .take_while(|&b| b != 0)
        .collect();
    if bytes.is_empty() {
        "unknown coli_v4 error".into()
    } else {
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

fn path_to_cstring(path: &Path) -> Result<CString> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        CString::new(path.as_os_str().as_bytes()).map_err(|_| {
            Error::invalid(format!(
                "model dir contains interior NUL: {}",
                path.display()
            ))
        })
    }
    #[cfg(not(unix))]
    {
        let s = path
            .to_str()
            .ok_or_else(|| Error::invalid(format!("non-UTF8 model dir: {}", path.display())))?;
        CString::new(s).map_err(|_| {
            Error::invalid(format!(
                "model dir contains interior NUL: {}",
                path.display()
            ))
        })
    }
}

/// Helper for tests: model path that is intentionally missing.
#[cfg(test)]
pub(crate) fn missing_model_path() -> PathBuf {
    PathBuf::from("/nonexistent/colibri-v4-model-for-ffi-tests")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::{ffi_available, ffi_link_available};

    #[test]
    fn link_available_when_feature_on() {
        assert!(ffi_link_available());
    }

    #[test]
    fn open_missing_model_errors() {
        let result = V4Engine::open(V4EngineOpenOptions::new(missing_model_path()));
        let Err(err) = result else {
            panic!("expected open to fail for missing model");
        };
        let msg = err.to_string();
        if force_process_from_env() {
            assert!(
                msg.contains("COLIBRI_FORCE_PROCESS") || msg.contains("force"),
                "unexpected: {msg}"
            );
            return;
        }
        assert!(!msg.is_empty(), "expected open error message");
    }

    #[test]
    fn ffi_available_respects_link() {
        // Without FORCE_PROCESS, available == linked.
        if !force_process_from_env() {
            assert_eq!(ffi_available(), ffi_link_available());
        }
    }
}
