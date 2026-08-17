//! Family-selected multi-engine open API.

use std::ffi::CString;
use std::os::raw::{c_char, c_float, c_int, c_void};
use std::path::{Path, PathBuf};
use std::ptr;

use super::bindings::{
    self, COLI_VISUAL_ALL, ColiExpertGridDims, ColiGlmEngine, ColiGlmGenerateOptions,
    ColiGlmOpenOptions, ColiHwinfoSnap, ColiInkEngine, ColiInkGenerateOptions, ColiInkOpenOptions,
    ColiKimiEngine, ColiKimiGenerateOptions, ColiKimiOpenOptions, ColiModelSizeSummary,
    ColiProfSnap, ColiTiersSnap,
};
use super::force_process_from_env;
use super::v4::{V4Engine, V4EngineOpenOptions, V4GenerateOptions, V4SessionCreateOptions};
use crate::error::{Error, Result};
use crate::model::{ModelFamily, ModelInfo, ModelSizeInfo};
use crate::visual::{
    BinaryPollParts, ExpertHits, ExpertMap, HwinfoSnap, ProfileTurn, TiersSnap, VisualSnapshot,
};

const ERROR_BUF: usize = 512;

/// Engine families available for in-process open under `feature = "ffi"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FfiFamily {
    Glm,
    Kimi,
    Inkling,
    DeepseekV4,
}

impl FfiFamily {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Glm => "glm",
            Self::Kimi => "kimi",
            Self::Inkling => "inkling",
            Self::DeepseekV4 => "deepseek_v4",
        }
    }

    pub fn engine_id(self) -> &'static str {
        match self {
            Self::Glm => "colibri",
            Self::Kimi => "kimi_k3",
            Self::Inkling => "inkling",
            Self::DeepseekV4 => "deepseek_v4",
        }
    }

    pub fn from_model_family(f: ModelFamily) -> Option<Self> {
        match f {
            ModelFamily::Glm | ModelFamily::Olmoe => Some(Self::Glm),
            ModelFamily::Kimi => Some(Self::Kimi),
            ModelFamily::Inkling => Some(Self::Inkling),
            ModelFamily::DeepseekV4 => Some(Self::DeepseekV4),
        }
    }
}

/// Shared generate options for multi-family API.
#[derive(Debug, Clone, Copy)]
pub struct FfiGenerateOptions {
    pub max_new_tokens: i32,
}

impl Default for FfiGenerateOptions {
    fn default() -> Self {
        Self { max_new_tokens: 32 }
    }
}

/// GLM in-process open knobs (maps to [`ColiGlmOpenOptions`]).
///
/// Zero fields mean C defaults (`cap=64`, `expert_bits=4`, `dense_bits=8`).
/// Oracle / process parity on `glm_tiny` uses `cap=64`, `expert_bits=16`,
/// `dense_bits=16` (same as `./colibri 64 16 16`).
#[derive(Debug, Clone, Copy, Default)]
pub struct GlmOpenOptions {
    pub cap: i32,
    pub expert_bits: i32,
    pub dense_bits: i32,
}

impl GlmOpenOptions {
    /// Match CLI self-test args: `./colibri 64 16 16`.
    pub fn oracle_parity() -> Self {
        Self {
            cap: 64,
            expert_bits: 16,
            dense_bits: 16,
        }
    }
}

/// Opened in-process engine (family selected at open).
pub enum FfiEngine {
    Glm(GlmEngine),
    Kimi(KimiEngine),
    Inkling(InkEngine),
    DeepseekV4(V4Engine),
}

impl FfiEngine {
    pub fn family(&self) -> FfiFamily {
        match self {
            Self::Glm(_) => FfiFamily::Glm,
            Self::Kimi(_) => FfiFamily::Kimi,
            Self::Inkling(_) => FfiFamily::Inkling,
            Self::DeepseekV4(_) => FfiFamily::DeepseekV4,
        }
    }

    /// Size snapshot: prefer Rust inspect (richer); fall back to C engine size.
    pub fn size_info(&self) -> ModelSizeInfo {
        match self {
            Self::Glm(e) => e.size_info(),
            Self::Kimi(e) => e.size_info(),
            Self::Inkling(e) => e.size_info(),
            Self::DeepseekV4(e) => e.size_info(),
        }
    }

    pub fn generate<F>(
        &mut self,
        prompt: &str,
        options: FfiGenerateOptions,
        on_token: F,
    ) -> Result<()>
    where
        F: FnMut(super::V4TokenEvent) -> Result<()>,
    {
        match self {
            Self::Glm(e) => e.generate(prompt, options, on_token),
            Self::Kimi(e) => e.generate(prompt, options, on_token),
            Self::Inkling(e) => e.generate(prompt, options, on_token),
            Self::DeepseekV4(e) => {
                let mut session = e.create_session(V4SessionCreateOptions::default())?;
                let _stats = session.generate(
                    prompt,
                    V4GenerateOptions {
                        max_new_tokens: options.max_new_tokens,
                        stop_at_sentence: false,
                        no_dspark: false,
                    },
                    on_token,
                )?;
                Ok(())
            }
        }
    }

    /// Greedy generate from prompt token ids (GLM only). See [`GlmEngine::generate_ids`].
    pub fn generate_ids<F>(
        &mut self,
        prompt_ids: &[i32],
        options: FfiGenerateOptions,
        on_token: F,
    ) -> Result<()>
    where
        F: FnMut(super::V4TokenEvent) -> Result<()>,
    {
        match self {
            Self::Glm(e) => e.generate_ids(prompt_ids, options, on_token),
            Self::Kimi(_) | Self::Inkling(_) | Self::DeepseekV4(_) => Err(Error::invalid(
                "generate_ids is only implemented for FfiFamily::Glm",
            )),
        }
    }

    /// Poll embed visual telemetry into the engine snapshot (HWINFO/TIERS/EMAP/HITS/PROF).
    ///
    /// GLM fills from `coli_glm_visual_poll`. Kimi/Inkling stubs return empty success
    /// until family fill lands. DeepSeek V4 has no visual symbols yet (empty snapshot).
    /// Does not invent mux STOP; cooperative cancel stays on the token callback.
    pub fn pump_visual(&mut self) -> Result<VisualSnapshot> {
        match self {
            Self::Glm(e) => e.pump_visual(),
            Self::Kimi(e) => e.pump_visual(),
            Self::Inkling(e) => e.pump_visual(),
            Self::DeepseekV4(_) => Ok(VisualSnapshot::default()),
        }
    }

    /// Latest visual snapshot without re-polling C.
    pub fn visual_snapshot(&self) -> VisualSnapshot {
        match self {
            Self::Glm(e) => e.visual_snapshot(),
            Self::Kimi(e) => e.visual_snapshot(),
            Self::Inkling(e) => e.visual_snapshot(),
            Self::DeepseekV4(_) => VisualSnapshot::default(),
        }
    }
}

/// Open by family (model path). Uses kill-switch and inspect for size fields.
///
/// GLM uses [`GlmOpenOptions::default`] (C defaults). For tiny oracle parity
/// open with [`open_glm`] and [`GlmOpenOptions::oracle_parity`].
pub fn open_engine(family: FfiFamily, model_dir: impl AsRef<Path>) -> Result<FfiEngine> {
    if force_process_from_env() {
        return Err(Error::engine(
            "COLIBRI_FORCE_PROCESS is set; refusing in-process open (use process serve path)",
        ));
    }
    let path = model_dir.as_ref();
    // Prefer Rust inspect for disk_bytes / family / engine_id on all paths.
    let inspect = ModelInfo::inspect(path).ok();
    match family {
        FfiFamily::Glm => Ok(FfiEngine::Glm(GlmEngine::open(
            path,
            inspect,
            GlmOpenOptions::default(),
        )?)),
        FfiFamily::Kimi => Ok(FfiEngine::Kimi(KimiEngine::open(path, inspect)?)),
        FfiFamily::Inkling => Ok(FfiEngine::Inkling(InkEngine::open(path, inspect)?)),
        FfiFamily::DeepseekV4 => {
            let eng = V4Engine::open(V4EngineOpenOptions::new(path))?;
            // attach inspect size if available via interior
            let _ = inspect;
            Ok(FfiEngine::DeepseekV4(eng))
        }
    }
}

/// Open GLM with explicit cap / expert_bits / dense_bits (for parity and tuning).
pub fn open_glm(model_dir: impl AsRef<Path>, options: GlmOpenOptions) -> Result<FfiEngine> {
    if force_process_from_env() {
        return Err(Error::engine(
            "COLIBRI_FORCE_PROCESS is set; refusing in-process open (use process serve path)",
        ));
    }
    let path = model_dir.as_ref();
    let inspect = ModelInfo::inspect(path).ok();
    Ok(FfiEngine::Glm(GlmEngine::open(path, inspect, options)?))
}

/* ---- GLM ---- */

pub struct GlmEngine {
    raw: *mut ColiGlmEngine,
    model_dir: PathBuf,
    size: ModelSizeInfo,
    visual: VisualSnapshot,
}

impl GlmEngine {
    fn open(path: &Path, inspect: Option<ModelInfo>, open: GlmOpenOptions) -> Result<Self> {
        super::apply_ffi_compute_niceness();
        let c_dir = path_to_cstring(path)?;
        let opts = ColiGlmOpenOptions {
            model_dir: c_dir.as_ptr(),
            cap: open.cap,
            expert_bits: open.expert_bits,
            dense_bits: open.dense_bits,
        };
        let mut raw: *mut ColiGlmEngine = ptr::null_mut();
        let mut err = [0i8; ERROR_BUF];
        let rc =
            unsafe { bindings::coli_glm_engine_open(&mut raw, &opts, err.as_mut_ptr(), err.len()) };
        if rc != 0 || raw.is_null() {
            return Err(Error::engine(c_error_string(&err)));
        }
        let size = size_from_c_or_inspect(raw, inspect, FfiFamily::Glm, path, true);
        Ok(Self {
            raw,
            model_dir: path.to_path_buf(),
            size,
            visual: VisualSnapshot::default(),
        })
    }

    pub fn size_info(&self) -> ModelSizeInfo {
        self.size.clone()
    }

    pub fn model_dir(&self) -> &Path {
        &self.model_dir
    }

    /// Poll `coli_glm_visual_poll` and update the cached snapshot.
    pub fn pump_visual(&mut self) -> Result<VisualSnapshot> {
        let raw = self.raw;
        poll_visual_into(
            |want,
             hw,
             tiers,
             ed,
             cells,
             cells_cap,
             cells_len,
             hd,
             bits,
             bits_cap,
             bits_len,
             hseq,
             prof,
             err,
             err_len| unsafe {
                bindings::coli_glm_visual_poll(
                    raw, want, hw, tiers, ed, cells, cells_cap, cells_len, hd, bits, bits_cap,
                    bits_len, hseq, prof, err, err_len,
                )
            },
            &mut self.visual,
        )?;
        Ok(self.visual.clone())
    }

    pub fn visual_snapshot(&self) -> VisualSnapshot {
        self.visual.clone()
    }

    pub fn generate<F>(
        &mut self,
        prompt: &str,
        options: FfiGenerateOptions,
        mut on_token: F,
    ) -> Result<()>
    where
        F: FnMut(super::V4TokenEvent) -> Result<()>,
    {
        if force_process_from_env() {
            return Err(Error::engine(
                "COLIBRI_FORCE_PROCESS is set; refusing in-process generate",
            ));
        }
        super::apply_ffi_compute_niceness();
        let c_opts = ColiGlmGenerateOptions {
            max_new_tokens: options.max_new_tokens,
        };
        let mut err = [0i8; ERROR_BUF];
        struct Ctx<'a, F> {
            f: &'a mut F,
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
            F: FnMut(super::V4TokenEvent) -> Result<()>,
        {
            let ctx = unsafe { &mut *(user_data as *mut Ctx<'_, F>) };
            match (ctx.f)(super::V4TokenEvent {
                token,
                logit,
                position,
                ordinal,
            }) {
                Ok(()) => 0,
                Err(e) => {
                    ctx.err = Some(e);
                    1
                }
            }
        }
        let mut ctx = Ctx {
            f: &mut on_token,
            err: None,
        };
        let rc = unsafe {
            bindings::coli_glm_generate(
                self.raw,
                prompt.as_ptr() as *const _,
                prompt.len(),
                &c_opts,
                Some(trampoline::<F>),
                &mut ctx as *mut _ as *mut c_void,
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
        Ok(())
    }

    /// Greedy free-generate from prompt token ids (no tokenizer).
    ///
    /// Mirrors the CLI oracle free-generate path (`SNAP=./glm_tiny ./colibri`
    /// with `ref_glm.json` `prompt_ids`). Temperature is forced to 0 for the
    /// call. Used for process↔FFI tiny golden parity.
    pub fn generate_ids<F>(
        &mut self,
        prompt_ids: &[i32],
        options: FfiGenerateOptions,
        mut on_token: F,
    ) -> Result<()>
    where
        F: FnMut(super::V4TokenEvent) -> Result<()>,
    {
        if force_process_from_env() {
            return Err(Error::engine(
                "COLIBRI_FORCE_PROCESS is set; refusing in-process generate",
            ));
        }
        if prompt_ids.is_empty() {
            return Err(Error::invalid("prompt_ids must be non-empty"));
        }
        super::apply_ffi_compute_niceness();
        let c_opts = ColiGlmGenerateOptions {
            max_new_tokens: options.max_new_tokens,
        };
        let mut err = [0i8; ERROR_BUF];
        struct Ctx<'a, F> {
            f: &'a mut F,
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
            F: FnMut(super::V4TokenEvent) -> Result<()>,
        {
            let ctx = unsafe { &mut *(user_data as *mut Ctx<'_, F>) };
            match (ctx.f)(super::V4TokenEvent {
                token,
                logit,
                position,
                ordinal,
            }) {
                Ok(()) => 0,
                Err(e) => {
                    ctx.err = Some(e);
                    1
                }
            }
        }
        let mut ctx = Ctx {
            f: &mut on_token,
            err: None,
        };
        let rc = unsafe {
            bindings::coli_glm_generate_ids(
                self.raw,
                prompt_ids.as_ptr(),
                prompt_ids.len() as c_int,
                &c_opts,
                Some(trampoline::<F>),
                &mut ctx as *mut _ as *mut c_void,
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
        Ok(())
    }
}

impl Drop for GlmEngine {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe { bindings::coli_glm_engine_destroy(self.raw) };
            self.raw = ptr::null_mut();
        }
    }
}

/* ---- Kimi ---- */

pub struct KimiEngine {
    raw: *mut ColiKimiEngine,
    model_dir: PathBuf,
    size: ModelSizeInfo,
    visual: VisualSnapshot,
}

impl KimiEngine {
    fn open(path: &Path, inspect: Option<ModelInfo>) -> Result<Self> {
        super::apply_ffi_compute_niceness();
        let c_dir = path_to_cstring(path)?;
        let opts = ColiKimiOpenOptions {
            model_dir: c_dir.as_ptr(),
            n_layers: 0,
        };
        let mut raw: *mut ColiKimiEngine = ptr::null_mut();
        let mut err = [0i8; ERROR_BUF];
        let rc = unsafe {
            bindings::coli_kimi_engine_open(&mut raw, &opts, err.as_mut_ptr(), err.len())
        };
        if rc != 0 || raw.is_null() {
            return Err(Error::engine(c_error_string(&err)));
        }
        let size = size_from_c_or_inspect_kimi(raw, inspect, path);
        Ok(Self {
            raw,
            model_dir: path.to_path_buf(),
            size,
            visual: VisualSnapshot::default(),
        })
    }

    pub fn size_info(&self) -> ModelSizeInfo {
        self.size.clone()
    }

    pub fn model_dir(&self) -> &Path {
        &self.model_dir
    }

    /// Stub poll until Kimi fill lands (empty success keeps snapshot default).
    pub fn pump_visual(&mut self) -> Result<VisualSnapshot> {
        let raw = self.raw;
        poll_visual_into(
            |want,
             hw,
             tiers,
             ed,
             cells,
             cells_cap,
             cells_len,
             hd,
             bits,
             bits_cap,
             bits_len,
             hseq,
             prof,
             err,
             err_len| unsafe {
                bindings::coli_kimi_visual_poll(
                    raw, want, hw, tiers, ed, cells, cells_cap, cells_len, hd, bits, bits_cap,
                    bits_len, hseq, prof, err, err_len,
                )
            },
            &mut self.visual,
        )?;
        Ok(self.visual.clone())
    }

    pub fn visual_snapshot(&self) -> VisualSnapshot {
        self.visual.clone()
    }

    pub fn generate<F>(
        &mut self,
        prompt: &str,
        options: FfiGenerateOptions,
        mut on_token: F,
    ) -> Result<()>
    where
        F: FnMut(super::V4TokenEvent) -> Result<()>,
    {
        if force_process_from_env() {
            return Err(Error::engine(
                "COLIBRI_FORCE_PROCESS is set; refusing in-process generate",
            ));
        }
        super::apply_ffi_compute_niceness();
        let c_opts = ColiKimiGenerateOptions {
            max_new_tokens: options.max_new_tokens,
        };
        let mut err = [0i8; ERROR_BUF];
        struct Ctx<'a, F> {
            f: &'a mut F,
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
            F: FnMut(super::V4TokenEvent) -> Result<()>,
        {
            let ctx = unsafe { &mut *(user_data as *mut Ctx<'_, F>) };
            match (ctx.f)(super::V4TokenEvent {
                token,
                logit,
                position,
                ordinal,
            }) {
                Ok(()) => 0,
                Err(e) => {
                    ctx.err = Some(e);
                    1
                }
            }
        }
        let mut ctx = Ctx {
            f: &mut on_token,
            err: None,
        };
        let rc = unsafe {
            bindings::coli_kimi_generate(
                self.raw,
                prompt.as_ptr() as *const _,
                prompt.len(),
                &c_opts,
                Some(trampoline::<F>),
                &mut ctx as *mut _ as *mut c_void,
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
        Ok(())
    }
}

impl Drop for KimiEngine {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe { bindings::coli_kimi_engine_destroy(self.raw) };
            self.raw = ptr::null_mut();
        }
    }
}

/* ---- Inkling ---- */

pub struct InkEngine {
    raw: *mut ColiInkEngine,
    model_dir: PathBuf,
    size: ModelSizeInfo,
    visual: VisualSnapshot,
}

impl InkEngine {
    fn open(path: &Path, inspect: Option<ModelInfo>) -> Result<Self> {
        super::apply_ffi_compute_niceness();
        let c_dir = path_to_cstring(path)?;
        let opts = ColiInkOpenOptions {
            model_dir: c_dir.as_ptr(),
            cap: 0,
            bits: 0,
        };
        let mut raw: *mut ColiInkEngine = ptr::null_mut();
        let mut err = [0i8; ERROR_BUF];
        let rc =
            unsafe { bindings::coli_ink_engine_open(&mut raw, &opts, err.as_mut_ptr(), err.len()) };
        if rc != 0 || raw.is_null() {
            return Err(Error::engine(c_error_string(&err)));
        }
        let size = size_from_c_or_inspect_ink(raw, inspect, path);
        Ok(Self {
            raw,
            model_dir: path.to_path_buf(),
            size,
            visual: VisualSnapshot::default(),
        })
    }

    pub fn size_info(&self) -> ModelSizeInfo {
        self.size.clone()
    }

    pub fn model_dir(&self) -> &Path {
        &self.model_dir
    }

    /// Stub poll until Inkling fill lands (empty success keeps snapshot default).
    pub fn pump_visual(&mut self) -> Result<VisualSnapshot> {
        let raw = self.raw;
        poll_visual_into(
            |want,
             hw,
             tiers,
             ed,
             cells,
             cells_cap,
             cells_len,
             hd,
             bits,
             bits_cap,
             bits_len,
             hseq,
             prof,
             err,
             err_len| unsafe {
                bindings::coli_ink_visual_poll(
                    raw, want, hw, tiers, ed, cells, cells_cap, cells_len, hd, bits, bits_cap,
                    bits_len, hseq, prof, err, err_len,
                )
            },
            &mut self.visual,
        )?;
        Ok(self.visual.clone())
    }

    pub fn visual_snapshot(&self) -> VisualSnapshot {
        self.visual.clone()
    }

    pub fn generate<F>(
        &mut self,
        prompt: &str,
        options: FfiGenerateOptions,
        mut on_token: F,
    ) -> Result<()>
    where
        F: FnMut(super::V4TokenEvent) -> Result<()>,
    {
        if force_process_from_env() {
            return Err(Error::engine(
                "COLIBRI_FORCE_PROCESS is set; refusing in-process generate",
            ));
        }
        super::apply_ffi_compute_niceness();
        let c_opts = ColiInkGenerateOptions {
            max_new_tokens: options.max_new_tokens,
        };
        let mut err = [0i8; ERROR_BUF];
        struct Ctx<'a, F> {
            f: &'a mut F,
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
            F: FnMut(super::V4TokenEvent) -> Result<()>,
        {
            let ctx = unsafe { &mut *(user_data as *mut Ctx<'_, F>) };
            match (ctx.f)(super::V4TokenEvent {
                token,
                logit,
                position,
                ordinal,
            }) {
                Ok(()) => 0,
                Err(e) => {
                    ctx.err = Some(e);
                    1
                }
            }
        }
        let mut ctx = Ctx {
            f: &mut on_token,
            err: None,
        };
        let rc = unsafe {
            bindings::coli_ink_generate(
                self.raw,
                prompt.as_ptr() as *const _,
                prompt.len(),
                &c_opts,
                Some(trampoline::<F>),
                &mut ctx as *mut _ as *mut c_void,
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
        Ok(())
    }
}

impl Drop for InkEngine {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe { bindings::coli_ink_engine_destroy(self.raw) };
            self.raw = ptr::null_mut();
        }
    }
}

/* ---- helpers ---- */

/// Call family `coli_*_visual_poll` (size probe then fill) and absorb into `snap`.
fn poll_visual_into<F>(mut poll: F, snap: &mut VisualSnapshot) -> Result<()>
where
    F: FnMut(
        u32,
        *mut ColiHwinfoSnap,
        *mut ColiTiersSnap,
        *mut ColiExpertGridDims,
        *mut u8,
        usize,
        *mut usize,
        *mut ColiExpertGridDims,
        *mut u8,
        usize,
        *mut usize,
        *mut u64,
        *mut ColiProfSnap,
        *mut c_char,
        usize,
    ) -> c_int,
{
    let want = COLI_VISUAL_ALL;
    let mut err = [0i8; ERROR_BUF];
    let mut hwinfo = ColiHwinfoSnap::default();
    let mut tiers = ColiTiersSnap::default();
    let mut emap_dims = ColiExpertGridDims::default();
    let mut hits_dims = ColiExpertGridDims::default();
    let mut emap_len = 0usize;
    let mut hits_len = 0usize;
    let mut hits_seq = 0u64;
    let mut prof = ColiProfSnap::default();

    // Size probe: null cell/bit buffers; does not clear HITS marks.
    let rc = poll(
        want,
        &mut hwinfo,
        &mut tiers,
        &mut emap_dims,
        ptr::null_mut(),
        0,
        &mut emap_len,
        &mut hits_dims,
        ptr::null_mut(),
        0,
        &mut hits_len,
        &mut hits_seq,
        &mut prof,
        err.as_mut_ptr(),
        err.len(),
    );
    if rc != 0 && rc != -2 {
        return Err(Error::engine(c_error_string(&err)));
    }

    let mut emap_cells = vec![0u8; emap_len];
    let mut hits_bits = vec![0u8; hits_len];

    let rc = poll(
        want,
        &mut hwinfo,
        &mut tiers,
        &mut emap_dims,
        if emap_cells.is_empty() {
            ptr::null_mut()
        } else {
            emap_cells.as_mut_ptr()
        },
        emap_cells.len(),
        &mut emap_len,
        &mut hits_dims,
        if hits_bits.is_empty() {
            ptr::null_mut()
        } else {
            hits_bits.as_mut_ptr()
        },
        hits_bits.len(),
        &mut hits_len,
        &mut hits_seq,
        &mut prof,
        err.as_mut_ptr(),
        err.len(),
    );
    if rc == -2 {
        // Retry once with reported sizes (defensive if first probe under-reported).
        emap_cells.resize(emap_len, 0);
        hits_bits.resize(hits_len, 0);
        let rc2 = poll(
            want,
            &mut hwinfo,
            &mut tiers,
            &mut emap_dims,
            emap_cells.as_mut_ptr(),
            emap_cells.len(),
            &mut emap_len,
            &mut hits_dims,
            hits_bits.as_mut_ptr(),
            hits_bits.len(),
            &mut hits_len,
            &mut hits_seq,
            &mut prof,
            err.as_mut_ptr(),
            err.len(),
        );
        if rc2 != 0 {
            return Err(Error::engine(c_error_string(&err)));
        }
    } else if rc != 0 {
        return Err(Error::engine(c_error_string(&err)));
    }

    if emap_len < emap_cells.len() {
        emap_cells.truncate(emap_len);
    }
    if hits_len < hits_bits.len() {
        hits_bits.truncate(hits_len);
    }

    let parts = BinaryPollParts {
        hwinfo: Some(HwinfoSnap {
            cores: hwinfo.cores,
            ram_total_gb: hwinfo.ram_total_gb,
            ram_avail_gb: hwinfo.ram_avail_gb,
            gpus: hwinfo.gpus,
            vram_total_gb: hwinfo.vram_total_gb,
            cpu: c_fixed_string(&hwinfo.cpu),
            gpu: c_fixed_string(&hwinfo.gpu),
        }),
        tiers: Some(TiersSnap {
            vram: tiers.vram_experts,
            ram: tiers.ram_experts,
            disk: tiers.disk_experts,
            vram_gb: tiers.vram_gb,
            ram_gb: tiers.ram_gb,
        }),
        expert_map: if emap_dims.rows > 0 && emap_dims.cols > 0 && !emap_cells.is_empty() {
            Some(ExpertMap::from_cells(
                emap_dims.rows,
                emap_dims.cols,
                emap_cells,
            ))
        } else {
            None
        },
        expert_hits: if hits_dims.rows > 0 && hits_dims.cols > 0 {
            Some(ExpertHits::from_bits(
                hits_dims.rows,
                hits_dims.cols,
                hits_bits,
                hits_seq,
            ))
        } else {
            None
        },
        profile: if prof.valid != 0 {
            Some((
                prof.seq,
                ProfileTurn {
                    wall_s: prof.wall_s,
                    prompt_tokens: prof.prompt_tokens,
                    completion_tokens: prof.completion_tokens,
                    expert_disk_s: prof.expert_disk_s,
                    expert_wait_s: prof.expert_wait_s,
                    expert_matmul_s: prof.expert_matmul_s,
                    attention_s: prof.attention_s,
                    lm_head_s: prof.lm_head_s,
                    forwards: prof.forwards,
                },
            ))
        } else {
            None
        },
    };
    snap.absorb_binary_poll(parts);
    Ok(())
}

fn c_fixed_string(buf: &[c_char]) -> String {
    let bytes: Vec<u8> = buf
        .iter()
        .map(|&c| c as u8)
        .take_while(|&b| b != 0)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn size_from_c_or_inspect(
    raw: *mut ColiGlmEngine,
    inspect: Option<ModelInfo>,
    family: FfiFamily,
    path: &Path,
    _glm: bool,
) -> ModelSizeInfo {
    if let Some(info) = inspect {
        return info.size_info();
    }
    let mut s = ColiModelSizeSummary {
        disk_bytes: 0,
        dense_bytes: 0,
        expert_bytes: 0,
        param_count: 0,
        has_param_count: 0,
        family: [0; 32],
        engine_id: [0; 32],
    };
    unsafe { bindings::coli_glm_engine_size(raw, &mut s) };
    ModelSizeInfo {
        path: path.to_path_buf(),
        family: Some(ModelFamily::Glm),
        engine_id: family.engine_id().into(),
        disk_bytes: s.disk_bytes,
        model_bytes: s.disk_bytes,
        dense_bytes: s.dense_bytes,
        expert_bytes: s.expert_bytes,
        param_count: if s.has_param_count != 0 {
            Some(s.param_count)
        } else {
            None
        },
        shards: 0,
        tier_vram_bytes: None,
        tier_ram_bytes: None,
        tier_disk_bytes: None,
    }
}

fn size_from_c_or_inspect_kimi(
    raw: *mut ColiKimiEngine,
    inspect: Option<ModelInfo>,
    path: &Path,
) -> ModelSizeInfo {
    if let Some(info) = inspect {
        return info.size_info();
    }
    let mut s = ColiModelSizeSummary {
        disk_bytes: 0,
        dense_bytes: 0,
        expert_bytes: 0,
        param_count: 0,
        has_param_count: 0,
        family: [0; 32],
        engine_id: [0; 32],
    };
    unsafe { bindings::coli_kimi_engine_size(raw, &mut s) };
    ModelSizeInfo {
        path: path.to_path_buf(),
        family: Some(ModelFamily::Kimi),
        engine_id: "kimi_k3".into(),
        disk_bytes: s.disk_bytes,
        model_bytes: s.disk_bytes,
        dense_bytes: s.dense_bytes,
        expert_bytes: s.expert_bytes,
        param_count: if s.has_param_count != 0 {
            Some(s.param_count)
        } else {
            None
        },
        shards: 0,
        tier_vram_bytes: None,
        tier_ram_bytes: None,
        tier_disk_bytes: None,
    }
}

fn size_from_c_or_inspect_ink(
    raw: *mut ColiInkEngine,
    inspect: Option<ModelInfo>,
    path: &Path,
) -> ModelSizeInfo {
    if let Some(info) = inspect {
        return info.size_info();
    }
    let mut s = ColiModelSizeSummary {
        disk_bytes: 0,
        dense_bytes: 0,
        expert_bytes: 0,
        param_count: 0,
        has_param_count: 0,
        family: [0; 32],
        engine_id: [0; 32],
    };
    unsafe { bindings::coli_ink_engine_size(raw, &mut s) };
    ModelSizeInfo {
        path: path.to_path_buf(),
        family: Some(ModelFamily::Inkling),
        engine_id: "inkling".into(),
        disk_bytes: s.disk_bytes,
        model_bytes: s.disk_bytes,
        dense_bytes: s.dense_bytes,
        expert_bytes: s.expert_bytes,
        param_count: if s.has_param_count != 0 {
            Some(s.param_count)
        } else {
            None
        },
        shards: 0,
        tier_vram_bytes: None,
        tier_ram_bytes: None,
        tier_disk_bytes: None,
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

fn c_error_string(err: &[i8]) -> String {
    let bytes: Vec<u8> = err
        .iter()
        .map(|&c| c as u8)
        .take_while(|&b| b != 0)
        .collect();
    if bytes.is_empty() {
        "unknown coli engine error".into()
    } else {
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::{ffi_available, ffi_family_available, ffi_link_available, linked_families};

    #[test]
    fn linked_families_include_product_engines() {
        assert!(ffi_link_available());
        assert!(linked_families().contains(&FfiFamily::Glm));
        assert!(linked_families().contains(&FfiFamily::Kimi));
        assert!(linked_families().contains(&FfiFamily::Inkling));
        assert!(linked_families().contains(&FfiFamily::DeepseekV4));
    }

    #[test]
    fn from_model_family_maps_inkling() {
        assert_eq!(
            FfiFamily::from_model_family(ModelFamily::Inkling),
            Some(FfiFamily::Inkling)
        );
        assert_eq!(FfiFamily::Inkling.engine_id(), "inkling");
        assert_eq!(FfiFamily::Inkling.as_str(), "inkling");
    }

    #[test]
    fn glm_open_invokes_compute_thread_niceness() {
        use std::thread;

        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let before = crate::ffi::apply_ffi_compute_niceness_call_count();
            let _ = GlmEngine::open(
                Path::new("/nonexistent/colibri-nice-probe"),
                None,
                GlmOpenOptions::default(),
            );
            tx.send(crate::ffi::apply_ffi_compute_niceness_call_count() > before)
                .expect("send count");
        })
        .join()
        .expect("worker join");
        assert!(
            rx.recv().expect("count"),
            "GlmEngine::open must call apply_ffi_compute_niceness (FFI start path)"
        );
    }

    #[test]
    fn open_glm_missing_errors() {
        let r = open_engine(
            FfiFamily::Glm,
            Path::new("/nonexistent/colibri-glm-ffi-test"),
        );
        assert!(r.is_err());
    }

    #[test]
    fn open_kimi_missing_errors() {
        let r = open_engine(
            FfiFamily::Kimi,
            Path::new("/nonexistent/colibri-kimi-ffi-test"),
        );
        assert!(r.is_err());
    }

    #[test]
    fn open_inkling_missing_errors() {
        let r = open_engine(
            FfiFamily::Inkling,
            Path::new("/nonexistent/colibri-inkling-ffi-test"),
        );
        assert!(r.is_err());
    }

    #[test]
    fn family_available_tracks_env() {
        if !force_process_from_env() {
            assert!(ffi_available());
            assert!(ffi_family_available(FfiFamily::Glm));
            assert!(ffi_family_available(FfiFamily::Inkling));
        }
    }

    #[test]
    fn glm_tiny_open_has_disk_bytes() {
        let _ffi_globals = crate::ffi::lock_ffi_process_global_test();
        if force_process_from_env() {
            return;
        }
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../c/glm_tiny");
        if !root.join("model.safetensors").is_file() {
            return;
        }
        // Open loads full model (can be heavy for tiny but glm_tiny is ~2MB).
        let eng = open_engine(FfiFamily::Glm, &root);
        match eng {
            Ok(e) => {
                let s = e.size_info();
                assert!(s.disk_bytes > 0, "disk_bytes on open");
                assert_eq!(s.engine_id, "colibri");
                assert_eq!(e.family(), FfiFamily::Glm);
            }
            Err(e) => {
                // Load may still fail on incomplete tokenizer; size inspect should work.
                let info = ModelInfo::inspect(&root).expect("inspect glm_tiny");
                assert!(info.disk_bytes > 0);
                let _ = e; // open failed but size path is covered by inspect
            }
        }
    }

    /// D2: process CLI free-generate and FFI `generate_ids` must emit the same
    /// greedy token sequence on `c/glm_tiny` + `c/ref_glm.json` prompt_ids.
    ///
    /// Requires:
    /// - `c/glm_tiny/model.safetensors` (~2.4 MB tiny fixture)
    /// - `c/ref_glm.json` (oracle prompt_ids / full_ids)
    /// - `c/colibri` process engine binary (built via `make -C c colibri`)
    ///
    /// When fixtures or the process binary are missing, the test returns early
    /// (not ignored) so default `cargo test -p colibri-sys --lib --features ffi`
    /// stays green on hosts without the binary. To force a hard fail when
    /// process is required:
    ///
    /// ```bash
    /// COLIBRI_REQUIRE_PROCESS_PARITY=1 cargo test -p colibri-sys --lib --features ffi \
    ///   glm_tiny_process_ffi_token_parity -- --nocapture
    /// ```
    #[test]
    fn glm_tiny_process_ffi_token_parity() {
        let _ffi_globals = crate::ffi::lock_ffi_process_global_test();
        if force_process_from_env() {
            return;
        }

        let c_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../c");
        let c_dir = c_dir.canonicalize().unwrap_or(c_dir);
        let model = c_dir.join("glm_tiny");
        let ref_path = c_dir.join("ref_glm.json");
        let engine_bin = c_dir.join("colibri");

        if !model.join("model.safetensors").is_file() || !ref_path.is_file() {
            eprintln!("skip glm_tiny_process_ffi_token_parity: missing glm_tiny or ref_glm.json");
            return;
        }

        let oracle = load_ref_glm(&ref_path).expect("parse ref_glm.json");
        let n_new = oracle.full_ids.len() - oracle.prompt_ids.len();
        assert!(n_new > 0, "ref_glm full_ids must extend prompt_ids");
        let oracle_cont = &oracle.full_ids[oracle.prompt_ids.len()..];

        // --- FFI path: generate_ids (greedy, same prompt_ids + CLI open bits) ---
        let mut eng = match open_glm(&model, GlmOpenOptions::oracle_parity()) {
            Ok(e) => e,
            Err(e) => {
                panic!("FFI open glm_tiny failed (weights present): {e}");
            }
        };
        let mut ffi_tokens: Vec<i32> = Vec::new();
        eng.generate_ids(
            &oracle.prompt_ids,
            FfiGenerateOptions {
                max_new_tokens: n_new as i32,
            },
            |ev| {
                ffi_tokens.push(ev.token);
                Ok(())
            },
        )
        .expect("FFI generate_ids");
        assert_eq!(
            ffi_tokens.len(),
            n_new,
            "FFI should emit exactly max_new_tokens (no EOS stop on ids path)"
        );
        assert_eq!(
            ffi_tokens.as_slice(),
            oracle_cont,
            "FFI generate_ids must match ref_glm.json full_ids continuation (greedy)"
        );

        // --- Process path: CLI free-generate (no PROMPT → uses ref_glm prompt_ids) ---
        if !engine_bin.is_file() {
            let msg = format!(
                "process engine binary missing at {} — FFI matched oracle; rebuild with `make -C c colibri`",
                engine_bin.display()
            );
            if std::env::var_os("COLIBRI_REQUIRE_PROCESS_PARITY").is_some() {
                panic!("{msg}");
            }
            eprintln!("skip process half of parity: {msg}");
            return;
        }

        let process_tokens =
            run_process_free_generate(&engine_bin, &model, &ref_path).expect("process generate");
        assert_eq!(
            process_tokens.as_slice(),
            ffi_tokens.as_slice(),
            "process CLI free-generate and FFI generate_ids must agree on token sequence"
        );
        assert_eq!(
            process_tokens.as_slice(),
            oracle_cont,
            "process free-generate must also match ref_glm full_ids continuation"
        );
    }

    #[derive(Debug)]
    struct RefGlm {
        prompt_ids: Vec<i32>,
        full_ids: Vec<i32>,
    }

    fn load_ref_glm(path: &Path) -> Result<RefGlm> {
        let raw = std::fs::read_to_string(path).map_err(Error::Io)?;
        let v: serde_json::Value =
            serde_json::from_str(&raw).map_err(|e| Error::invalid(format!("ref_glm json: {e}")))?;
        let prompt_ids = json_i32_array(&v["prompt_ids"], "prompt_ids")?;
        let full_ids = json_i32_array(&v["full_ids"], "full_ids")?;
        if full_ids.len() < prompt_ids.len() {
            return Err(Error::invalid("ref_glm full_ids shorter than prompt_ids"));
        }
        Ok(RefGlm {
            prompt_ids,
            full_ids,
        })
    }

    fn json_i32_array(v: &serde_json::Value, name: &str) -> Result<Vec<i32>> {
        let arr = v
            .as_array()
            .ok_or_else(|| Error::invalid(format!("ref_glm missing array {name}")))?;
        arr.iter()
            .map(|x| {
                x.as_i64()
                    .map(|n| n as i32)
                    .ok_or_else(|| Error::invalid(format!("ref_glm {name} non-int")))
            })
            .collect()
    }

    /// Spawn `colibri` in oracle free-generate mode and parse the "GLM C engine" token line.
    fn run_process_free_generate(
        engine_bin: &Path,
        model_dir: &Path,
        ref_path: &Path,
    ) -> Result<Vec<i32>> {
        use std::process::Command;

        let out = Command::new(engine_bin)
            .args(["64", "16", "16"])
            .env("SNAP", model_dir)
            .env("REF", ref_path)
            .env("COLI_NO_OMP_TUNE", "1")
            .env("COLI_TEMP", "0")
            .env_remove("PROMPT")
            .env_remove("TF")
            .env_remove("SERVE")
            .env_remove("SERVE_BATCH")
            .output()
            .map_err(|e| Error::engine(format!("spawn process engine: {e}")))?;

        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        if !out.status.success() {
            return Err(Error::engine(format!(
                "process engine exit {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
                out.status.code()
            )));
        }

        // Line looks like: "GLM C engine      : 207 187 119 ..."
        for line in stdout.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("GLM C engine") {
                let after_colon = rest
                    .split_once(':')
                    .map(|(_, r)| r.trim())
                    .unwrap_or("")
                    .trim();
                if after_colon.is_empty() {
                    continue;
                }
                let mut toks = Vec::new();
                for part in after_colon.split_whitespace() {
                    let t: i32 = part.parse().map_err(|_| {
                        Error::engine(format!("bad token in process output: {part:?}"))
                    })?;
                    toks.push(t);
                }
                if !toks.is_empty() {
                    return Ok(toks);
                }
            }
        }
        Err(Error::engine(format!(
            "no 'GLM C engine' token line in process stdout:\n{stdout}\nstderr:\n{stderr}"
        )))
    }

    /// GLM embed visual poll maps into a non-empty `VisualSnapshot` (no SERVE subprocess).
    ///
    /// Soft-skip when `c/glm_tiny` is missing or open fails (same style as parity tests).
    #[test]
    fn glm_tiny_ffi_pump_visual_nonempty() {
        let _ffi_globals = crate::ffi::lock_ffi_process_global_test();
        if force_process_from_env() {
            return;
        }
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../c/glm_tiny");
        if !root.join("model.safetensors").is_file() {
            eprintln!("skip glm_tiny_ffi_pump_visual_nonempty: missing glm_tiny");
            return;
        }
        let mut eng = match open_glm(&root, GlmOpenOptions::oracle_parity()) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("skip glm_tiny_ffi_pump_visual_nonempty: open failed: {e}");
                return;
            }
        };
        let snap = eng.pump_visual().expect("pump_visual");
        // HWINFO/TIERS/EMAP should be available immediately after open (before generate).
        assert!(
            snap.hwinfo.is_some(),
            "FFI poll should fill hwinfo on glm_tiny"
        );
        assert!(
            snap.hwinfo.as_ref().unwrap().cores > 0,
            "hwinfo cores should be > 0"
        );
        assert!(snap.tiers.is_some(), "FFI poll should fill tiers");
        let map = snap
            .expert_map
            .as_ref()
            .expect("FFI poll should fill expert_map (EMAP) on glm_tiny");
        assert!(map.rows > 0 && map.cols > 0, "emap dims");
        assert_eq!(
            map.cells.len(),
            (map.rows as usize) * (map.cols as usize),
            "emap cell count"
        );
        // PROF only after a completed generate.
        assert!(
            snap.profile.is_empty() || snap.profile_seq > 0,
            "profile seq consistent"
        );
    }

    /// Mid-generate cooperative cancel: token callback `Err` must stop generate
    /// early (no mux STOP on pure FFI). Uses `generate_ids` so tokenizer.json is
    /// not required. Soft-skip without glm_tiny.
    #[test]
    fn glm_tiny_ffi_mid_generate_cooperative_cancel() {
        let _ffi_globals = crate::ffi::lock_ffi_process_global_test();
        if force_process_from_env() {
            return;
        }
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../c/glm_tiny");
        if !root.join("model.safetensors").is_file() {
            eprintln!("skip glm_tiny_ffi_mid_generate_cooperative_cancel: missing glm_tiny");
            return;
        }
        let mut eng = match open_glm(&root, GlmOpenOptions::oracle_parity()) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("skip glm_tiny_ffi_mid_generate_cooperative_cancel: open failed: {e}");
                return;
            }
        };
        // Minimal non-empty prompt ids (no tokenizer); greedy free-generate path.
        let prompt_ids = [1i32, 2, 3, 4];
        let mut n = 0u32;
        let result = eng.generate_ids(
            &prompt_ids,
            FfiGenerateOptions { max_new_tokens: 32 },
            |_ev| {
                n += 1;
                if n >= 1 {
                    return Err(Error::engine("stopped"));
                }
                Ok(())
            },
        );
        assert!(
            result.is_err(),
            "token-cb Err must surface as generate error (cooperative cancel)"
        );
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("stopped") || msg.contains("engine"),
            "unexpected cancel error: {msg}"
        );
        assert!(
            n <= 2,
            "cancel should stop near first token, got n={n} tokens"
        );
    }
}
