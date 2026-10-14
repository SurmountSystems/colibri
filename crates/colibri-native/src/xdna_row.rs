//! One operational sentence per catalog id.
//!
//! When `COLI_XDNA=1`, an engine `main` that calls
//! `coli_xdna_engine_qualified_submit` submits one 64 by 64 by 64 matmul at
//! process start. That is not an expert GEMM and it is not token generate.
//! Whether this window can start the row is a separate fact. Image, Laya, and
//! GLiNER have no such call, so they stay on the no-program sentence.

/// Startup check. Not a claim that the model runs on the NPU.
const STARTUP_CHECK: &str = "NPU check at start, not a generate.";

/// Image, Laya, GLiNER, and any id whose engine does not call the submit.
const NO_PROGRAM: &str = "No NPU program for this model.";

/// Sentence for one id from `list_supported_models` / `supported_models`.
///
/// The check sentence is for every catalog id whose engine `main` calls
/// `coli_xdna_engine_qualified_submit`: `c/colibri.c`, `c/glm53.c`,
/// `c/inkling.c`, `c/kimi_k3.c`, `c/deepseek_v4.c`, `c/deepseek_v41.c`,
/// `c/olmoe.c`, `c/qwen36.c`, `c/qwen38.c`, and `c/mimo.c`.
pub fn xdna_row_note(catalog_id: &str) -> &'static str {
    if startup_check_id(catalog_id) {
        STARTUP_CHECK
    } else {
        NO_PROGRAM
    }
}

fn startup_check_id(catalog_id: &str) -> bool {
    matches!(
        catalog_id,
        "glm-5.2-colibri"
            | "glm-5.3"
            | "glm-5.3-flash-colibri"
            | "inkling-colibri"
            | "kimi-k3-colibri"
            | "deepseek-v4-colibri"
            | "deepseek-v4-flash-reap"
            | "deepseek-v4.1-flash-colibri"
            | "olmoe-colibri"
            | "qwen36-35b"
            | "qwen3-coder-30b"
            | "qwen3.8-27b-colibri"
            | "qwen3-coder-reap-25b-a3b-colibri"
            | "qwen3.8-2.4t-a95b"
            | "qwen38-flash-next"
            | "mimo-v2.6-flash"
            | "mimo-v2.6-pro"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kimi_row_names_the_startup_check() {
        assert_eq!(
            xdna_row_note("kimi-k3-colibri"),
            "NPU check at start, not a generate."
        );
    }

    #[test]
    fn image_row_has_no_npu_program() {
        assert_eq!(
            xdna_row_note("qwen-image-2.1-colibri"),
            "No NPU program for this model."
        );
    }

    #[test]
    fn notes_do_not_claim_ready_compatible_or_generate_on() {
        for note in [
            xdna_row_note("kimi-k3-colibri"),
            xdna_row_note("glm-5.3"),
            xdna_row_note("qwen-image-2.1-colibri"),
            xdna_row_note("qwen36-35b"),
            xdna_row_note("glm-5.3-flash-colibri"),
            xdna_row_note("not-a-catalog-id"),
        ] {
            assert!(!note.contains("ready"), "{note}");
            assert!(!note.contains("compatible"), "{note}");
            assert!(!note.contains("generate on"), "{note}");
        }
    }

    #[test]
    fn unstarted_row_with_an_engine_call_names_the_startup_check() {
        assert_eq!(
            xdna_row_note("glm-5.3-flash-colibri"),
            "NPU check at start, not a generate."
        );
        assert_eq!(
            xdna_row_note("qwen36-35b"),
            "NPU check at start, not a generate."
        );
        assert_eq!(
            xdna_row_note("deepseek-v4.1-flash-colibri"),
            "NPU check at start, not a generate."
        );
        assert_eq!(
            xdna_row_note("mimo-v2.6-pro"),
            "NPU check at start, not a generate."
        );
    }
}
