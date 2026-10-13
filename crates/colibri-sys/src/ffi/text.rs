//! CPU embed wrappers for the 2.0 text MoE families.
//!
//! GLM, Kimi, Inkling, and DeepSeek V4 stay in [`super::multi`] and [`super::v4`].
//! These six follow the same open, size, generate, and visual-poll shape.

use std::os::raw::{c_char, c_float, c_int, c_void};
use std::path::{Path, PathBuf};
use std::ptr;

use super::bindings;
use super::force_process_from_env;
use super::multi::{
    ERROR_BUF, FfiFamily, FfiGenerateOptions, c_error_string, path_to_cstring, poll_visual_into,
};
use crate::error::{Error, Result};
use crate::model::{ModelFamily, ModelInfo, ModelSizeInfo};
use crate::visual::VisualSnapshot;

macro_rules! text_engine {
    (
        $eng:ident,
        $raw:ident,
        $family:expr,
        $model_family:expr,
        $open_ty:ident,
        $gen_ty:ident,
        $open_fn:path,
        $destroy_fn:path,
        $size_fn:path,
        $gen_fn:path,
        $poll_fn:path,
        $build_opts:expr
    ) => {
        pub struct $eng {
            raw: *mut bindings::$raw,
            model_dir: PathBuf,
            size: ModelSizeInfo,
            visual: VisualSnapshot,
        }

        impl $eng {
            pub fn open(path: &Path, inspect: Option<ModelInfo>) -> Result<Self> {
                super::apply_ffi_compute_niceness();
                let c_dir = path_to_cstring(path)?;
                let opts = $build_opts(c_dir.as_ptr());
                let mut raw: *mut bindings::$raw = ptr::null_mut();
                let mut err = [0i8; ERROR_BUF];
                let rc = unsafe { $open_fn(&mut raw, &opts, err.as_mut_ptr(), err.len()) };
                if rc != 0 || raw.is_null() {
                    return Err(Error::engine(c_error_string(&err)));
                }
                let size = if let Some(info) = inspect {
                    info.size_info()
                } else {
                    let mut summary = bindings::ColiModelSizeSummary {
                        disk_bytes: 0,
                        dense_bytes: 0,
                        expert_bytes: 0,
                        param_count: 0,
                        has_param_count: 0,
                        family: [0; 32],
                        engine_id: [0; 32],
                    };
                    unsafe { $size_fn(raw, &mut summary) };
                    let id_bytes: Vec<u8> = summary
                        .engine_id
                        .iter()
                        .map(|&c| c as u8)
                        .take_while(|&b| b != 0)
                        .collect();
                    ModelSizeInfo {
                        path: path.to_path_buf(),
                        family: $model_family,
                        engine_id: if id_bytes.is_empty() {
                            $family.engine_id().into()
                        } else {
                            String::from_utf8_lossy(&id_bytes).into_owned()
                        },
                        disk_bytes: summary.disk_bytes,
                        model_bytes: summary.disk_bytes,
                        dense_bytes: summary.dense_bytes,
                        expert_bytes: summary.expert_bytes,
                        param_count: if summary.has_param_count != 0 {
                            Some(summary.param_count)
                        } else {
                            None
                        },
                        shards: 0,
                        tier_vram_bytes: None,
                        tier_ram_bytes: None,
                        tier_disk_bytes: None,
                    }
                };
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
                        $poll_fn(
                            raw, want, hw, tiers, ed, cells, cells_cap, cells_len, hd, bits,
                            bits_cap, bits_len, hseq, prof, err, err_len,
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
                let c_opts = bindings::$gen_ty {
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
                    $gen_fn(
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

        impl Drop for $eng {
            fn drop(&mut self) {
                if !self.raw.is_null() {
                    unsafe { $destroy_fn(self.raw) };
                    self.raw = ptr::null_mut();
                }
            }
        }
    };
}

text_engine!(
    Glm53Engine,
    ColiGlm53Engine,
    FfiFamily::Glm53,
    None,
    ColiGlm53OpenOptions,
    ColiGlm53GenerateOptions,
    bindings::coli_glm53_engine_open,
    bindings::coli_glm53_engine_destroy,
    bindings::coli_glm53_engine_size,
    bindings::coli_glm53_generate,
    bindings::coli_glm53_visual_poll,
    |model_dir: *const c_char| bindings::ColiGlm53OpenOptions { model_dir, cap: 0 }
);

text_engine!(
    OlmoeEngine,
    ColiOlmoeEngine,
    FfiFamily::Olmoe,
    Some(ModelFamily::Olmoe),
    ColiOlmoeOpenOptions,
    ColiOlmoeGenerateOptions,
    bindings::coli_olmoe_engine_open,
    bindings::coli_olmoe_engine_destroy,
    bindings::coli_olmoe_engine_size,
    bindings::coli_olmoe_generate,
    bindings::coli_olmoe_visual_poll,
    |model_dir: *const c_char| bindings::ColiOlmoeOpenOptions {
        model_dir,
        cap: 0,
        bits: 0
    }
);

text_engine!(
    Qwen36Engine,
    ColiQwen36Engine,
    FfiFamily::Qwen36,
    None,
    ColiQwen36OpenOptions,
    ColiQwen36GenerateOptions,
    bindings::coli_qwen36_engine_open,
    bindings::coli_qwen36_engine_destroy,
    bindings::coli_qwen36_engine_size,
    bindings::coli_qwen36_generate,
    bindings::coli_qwen36_visual_poll,
    |model_dir: *const c_char| bindings::ColiQwen36OpenOptions {
        model_dir,
        cap: 0,
        bits: 0
    }
);

text_engine!(
    Qwen38Engine,
    ColiQwen38Engine,
    FfiFamily::Qwen38,
    None,
    ColiQwen38OpenOptions,
    ColiQwen38GenerateOptions,
    bindings::coli_qwen38_engine_open,
    bindings::coli_qwen38_engine_destroy,
    bindings::coli_qwen38_engine_size,
    bindings::coli_qwen38_generate,
    bindings::coli_qwen38_visual_poll,
    |model_dir: *const c_char| bindings::ColiQwen38OpenOptions {
        model_dir,
        cap: 0,
        bits: 0
    }
);

text_engine!(
    DeepseekV41Engine,
    ColiV41Engine,
    FfiFamily::DeepseekV41,
    None,
    ColiV41OpenOptions,
    ColiV41GenerateOptions,
    bindings::coli_v41_engine_open,
    bindings::coli_v41_engine_destroy,
    bindings::coli_v41_engine_size,
    bindings::coli_v41_generate,
    bindings::coli_v41_visual_poll,
    |model_dir: *const c_char| bindings::ColiV41OpenOptions {
        model_dir,
        ecap: 0,
        engram_cache_rows: 0
    }
);

text_engine!(
    MimoEngine,
    ColiMimoEngine,
    FfiFamily::Mimo,
    None,
    ColiMimoOpenOptions,
    ColiMimoGenerateOptions,
    bindings::coli_mimo_engine_open,
    bindings::coli_mimo_engine_destroy,
    bindings::coli_mimo_engine_size,
    bindings::coli_mimo_generate,
    bindings::coli_mimo_visual_poll,
    |model_dir: *const c_char| bindings::ColiMimoOpenOptions { model_dir, cap: 0 }
);

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::super::multi::{FfiFamily, open_engine};

    fn assert_c_open_missing(family: FfiFamily, tag: &str) {
        let path = format!("/nonexistent/colibri-{tag}-ffi-test");
        let msg = match open_engine(family, Path::new(&path)) {
            Err(err) => err.to_string(),
            Ok(_) => panic!("{tag} open must fail on a missing directory"),
        };
        assert!(
            msg.contains("no safetensors weights found"),
            "{tag} open must be the C binding, got: {msg}"
        );
    }

    #[test]
    fn open_glm53_missing_dir_reports_missing_weights() {
        assert_c_open_missing(FfiFamily::Glm53, "glm53");
    }

    #[test]
    fn open_olmoe_missing_dir_reports_missing_weights() {
        assert_c_open_missing(FfiFamily::Olmoe, "olmoe");
    }

    #[test]
    fn open_qwen36_missing_dir_reports_missing_weights() {
        assert_c_open_missing(FfiFamily::Qwen36, "qwen36");
    }

    #[test]
    fn open_qwen38_missing_dir_reports_missing_weights() {
        assert_c_open_missing(FfiFamily::Qwen38, "qwen38");
    }

    #[test]
    fn open_deepseek_v41_missing_dir_reports_missing_weights() {
        assert_c_open_missing(FfiFamily::DeepseekV41, "deepseek-v41");
    }

    #[test]
    fn open_mimo_missing_dir_reports_missing_weights() {
        assert_c_open_missing(FfiFamily::Mimo, "mimo");
    }
}
