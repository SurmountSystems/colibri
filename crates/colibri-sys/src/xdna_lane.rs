//! Linux XDNA lane report and dispatch selection.
//!
//! The host does not link XRT. A default build stays on the normal path until
//! `COLI_XDNA` parses as a non-zero integer. The helper is `libcoli_xdna.so`
//! beside the executable, by absolute path. There is no PATH search.
//!
//! Dispatch selection on Linux accepts every model in the 2.0 roster. That is
//! not a generate, and it is not a throughput claim. The historical Windows
//! gate (GLM shared-expert only, one K/N shape) is not the Linux rule.
//!
//! Device nodes are whatever `/sys/class/accel` shows with driver `amdxdna`.
//! Absence is reported as absence. A node without a helper does not make the
//! lane available.

use std::path::{Path, PathBuf};

use crate::model::supported_models;

/// Linux helper file name. Looked up beside the executable, never on `PATH`.
pub const LINUX_HELPER_FILE: &str = "libcoli_xdna.so";

/// Windows helper file name. Kept so the DLL path stays the fallback it is.
pub const WINDOWS_HELPER_FILE: &str = "coli_xdna.dll";

/// Why a requested lane is not available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnavailableReason {
    /// `COLI_XDNA` requested the lane and no helper file was at the absolute path.
    HelperAbsent,
}

/// Honest device report. Not a generate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceReport {
    Present { node: String },
    Absent,
}

/// Lane report for one query.
///
/// `is_available` is true only when the lane was requested, a helper file
/// exists at an absolute path, and a device node was supplied. A missing
/// helper is [`LaneStatus::Unavailable`]. A missing device stays visible on
/// [`LaneStatus::HelperBound`] and does not count as available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaneStatus {
    /// `COLI_XDNA` was unset, empty, zero, or not an integer.
    Off,
    Unavailable {
        reason: UnavailableReason,
    },
    /// Helper file resolved. Device presence is separate. Not a generate.
    HelperBound {
        device: DeviceReport,
    },
}

impl LaneStatus {
    /// True only for a requested lane with a helper file and a device node.
    /// Never true when the helper is absent.
    pub fn is_available(&self) -> bool {
        matches!(
            self,
            LaneStatus::HelperBound {
                device: DeviceReport::Present { .. }
            }
        )
    }
}

/// Why Linux dispatch selection refused a model id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchRefusal {
    /// Not an id in the 2.0 roster.
    NotOnRoster,
    /// Historical Windows gate: not the GLM shared-expert shape.
    /// Linux selection does not return this for a roster model.
    NotOldGlmShape,
}

/// Selection result. `generated` is always false. Selection does not run work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchSelection {
    pub model_id: String,
    pub accepted: bool,
    pub roster_family: Option<&'static str>,
    pub refusal: Option<DispatchRefusal>,
    pub generated: bool,
}

/// `COLI_XDNA` requests the lane when it parses as a non-zero integer.
pub fn coli_xdna_requests_lane(value: Option<&str>) -> bool {
    let Some(raw) = value else {
        return false;
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return false;
    }
    match trimmed.parse::<i64>() {
        Ok(n) => n != 0,
        Err(_) => false,
    }
}

/// Absolute path of the Linux helper beside `executable`.
/// `None` when `executable` has no parent directory.
pub fn linux_helper_beside(executable: &Path) -> Option<PathBuf> {
    Some(executable.parent()?.join(LINUX_HELPER_FILE))
}

/// Report the lane. `device_nodes` is an already-probed list (see
/// [`live_amdxdna_nodes`]). This function does not open a device and does not
/// generate. A relative helper path is treated as absent so the report cannot
/// imply a PATH search.
pub fn lane_status(
    coli_xdna: Option<&str>,
    helper: Option<&Path>,
    device_nodes: &[String],
) -> LaneStatus {
    if !coli_xdna_requests_lane(coli_xdna) {
        return LaneStatus::Off;
    }
    let Some(path) = helper else {
        return LaneStatus::Unavailable {
            reason: UnavailableReason::HelperAbsent,
        };
    };
    if !path.is_absolute() || !path.is_file() {
        return LaneStatus::Unavailable {
            reason: UnavailableReason::HelperAbsent,
        };
    }
    let device = match device_nodes.iter().find(|n| !n.is_empty()) {
        Some(node) => DeviceReport::Present { node: node.clone() },
        None => DeviceReport::Absent,
    };
    LaneStatus::HelperBound { device }
}

/// Same as [`lane_status`], probing this machine's amdxdna nodes.
pub fn lane_status_live(coli_xdna: Option<&str>, helper: Option<&Path>) -> LaneStatus {
    let nodes = live_amdxdna_nodes();
    lane_status(coli_xdna, helper, &nodes)
}

/// Accel nodes whose sysfs driver is `amdxdna`. Empty when the class is
/// missing or no node matches. Does not open the node.
pub fn live_amdxdna_nodes() -> Vec<String> {
    #[cfg(unix)]
    {
        scan_amdxdna_nodes()
    }
    #[cfg(not(unix))]
    {
        Vec::new()
    }
}

#[cfg(unix)]
fn scan_amdxdna_nodes() -> Vec<String> {
    let class = Path::new("/sys/class/accel");
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(class) else {
        return out;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name.contains('/') {
            continue;
        }
        let driver = class.join(name.as_ref()).join("device/driver");
        let Ok(target) = std::fs::read_link(&driver) else {
            continue;
        };
        if target.file_name().and_then(|s| s.to_str()) != Some("amdxdna") {
            continue;
        }
        let node = Path::new("/dev/accel").join(name.as_ref());
        let is_char = std::fs::metadata(&node)
            .map(|m| std::os::unix::fs::FileTypeExt::is_char_device(&m.file_type()))
            .unwrap_or(false);
        if is_char {
            out.push(node.to_string_lossy().into_owned());
        }
    }
    out
}

/// Linux dispatch selection. Every 2.0 roster id is accepted. An id that is
/// not on the roster is refused as [`DispatchRefusal::NotOnRoster`], not as
/// the old GLM shape gate. `generated` is always false.
pub fn linux_dispatch_selection(model_id: &str) -> DispatchSelection {
    match supported_models().iter().find(|m| m.id == model_id) {
        Some(model) => DispatchSelection {
            model_id: model.id.to_string(),
            accepted: true,
            roster_family: Some(model.roster_family),
            refusal: None,
            generated: false,
        },
        None => DispatchSelection {
            model_id: model_id.to_string(),
            accepted: false,
            roster_family: None,
            refusal: Some(DispatchRefusal::NotOnRoster),
            generated: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lane_is_unavailable_without_a_helper() {
        let missing = Path::new("/no/such/colibri-xdna-helper/libcoli_xdna.so");
        let status = lane_status(Some("1"), Some(missing), &[]);
        assert!(
            !status.is_available(),
            "lane reported available with no helper: {status:?}"
        );
        assert_eq!(
            status,
            LaneStatus::Unavailable {
                reason: UnavailableReason::HelperAbsent
            }
        );

        let with_node = lane_status(Some("1"), None, &["/dev/accel/accel0".to_string()]);
        assert!(
            !with_node.is_available(),
            "a device node must not make the lane available with no helper"
        );
        assert_eq!(
            with_node,
            LaneStatus::Unavailable {
                reason: UnavailableReason::HelperAbsent
            }
        );

        let relative = lane_status(Some("1"), Some(Path::new("libcoli_xdna.so")), &[]);
        assert!(!relative.is_available());
        assert_eq!(
            relative,
            LaneStatus::Unavailable {
                reason: UnavailableReason::HelperAbsent
            }
        );

        assert_eq!(lane_status(None, None, &[]), LaneStatus::Off);
        assert_eq!(lane_status(Some("0"), None, &[]), LaneStatus::Off);
        assert_eq!(lane_status(Some(""), None, &[]), LaneStatus::Off);
        assert_eq!(lane_status(Some("no"), None, &[]), LaneStatus::Off);
        assert!(!lane_status(None, None, &[]).is_available());
    }

    #[test]
    fn non_glm_model_is_accepted_by_linux_dispatch() {
        let kimi = linux_dispatch_selection("kimi-k3-colibri");
        assert!(
            kimi.accepted,
            "non-GLM model refused solely as {:?}",
            kimi.refusal
        );
        assert_ne!(kimi.refusal, Some(DispatchRefusal::NotOldGlmShape));
        assert_eq!(kimi.roster_family, Some("kimi"));
        assert!(!kimi.generated);

        for model in supported_models() {
            let decision = linux_dispatch_selection(model.id);
            assert!(
                decision.accepted,
                "{} refused as {:?} (old GLM shape gate)",
                model.id, decision.refusal
            );
            assert_ne!(decision.refusal, Some(DispatchRefusal::NotOldGlmShape));
            assert_eq!(decision.roster_family, Some(model.roster_family));
            assert!(
                !decision.generated,
                "{id} claimed a generate",
                id = model.id
            );
        }

        let unknown = linux_dispatch_selection("not-a-roster-model");
        assert!(!unknown.accepted);
        assert_eq!(unknown.refusal, Some(DispatchRefusal::NotOnRoster));
        assert!(!unknown.generated);
    }

    #[test]
    fn live_probe_does_not_generate() {
        let nodes = live_amdxdna_nodes();
        for node in &nodes {
            let meta = std::fs::metadata(node).expect(node);
            #[cfg(unix)]
            assert!(
                std::os::unix::fs::FileTypeExt::is_char_device(&meta.file_type()),
                "{node}"
            );
        }
        let status = lane_status_live(Some("1"), None);
        assert!(!status.is_available());
        assert_eq!(
            status,
            LaneStatus::Unavailable {
                reason: UnavailableReason::HelperAbsent
            }
        );
    }

    /// A non-GLM roster model must have a real executable path on this NPU.
    /// Name acceptance is not this test. While the registry only has the GLM
    /// K=6144 N=2048 buckets, kimi is declined for lack of an artifact and
    /// this test fails. It passes only when the qualified op completes on
    /// the device and the int32 output matches the host reference.
    #[cfg(target_os = "linux")]
    #[test]
    fn non_glm_roster_model_runs_qualified_op_on_npu() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let root = root.canonicalize().expect("repo root");
        let bin = std::env::temp_dir().join("coli-xdna-roster-op");
        let compile = std::process::Command::new("cc")
            .arg("-std=c11")
            .arg("-D_GNU_SOURCE")
            .arg("-O2")
            .arg("-Wall")
            .arg("-Wextra")
            .arg("-I")
            .arg(root.join("c"))
            .arg(root.join("c/backend_xdna.c"))
            .arg(root.join("c/xdna/roster_op_check.c"))
            .arg("-o")
            .arg(&bin)
            .status()
            .expect("cc");
        assert!(compile.success(), "roster checker failed to compile");

        let helper = root.join("c/xdna/libcoli_xdna.so");
        let artifacts = root.join("c/xdna/kernels");
        let output = std::process::Command::new(&bin)
            .arg(&helper)
            .arg(&artifacts)
            .env("COLI_XDNA", "1")
            .output()
            .expect("roster checker");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "qualified op did not run. A missing artifact declines; a name check is not a run.\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
        assert!(
            stdout.contains("model=kimi-k3-colibri executed=1 matched=1 node=/dev/accel/accel0"),
            "{stdout}"
        );
        assert!(
            !stdout.contains("executed=0"),
            "a roster family declined:\n{stdout}"
        );
    }

    /// The owned engine calls coli_xdna_engine_qualified_submit, which submits
    /// the 64x64x64 int16 program. The checker compares the device int32 buffer
    /// to the host reference. This is not a token generate.
    #[cfg(target_os = "linux")]
    #[test]
    fn engine_qualified_submit_reaches_the_npu() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let root = root.canonicalize().expect("repo root");
        let bin = std::env::temp_dir().join("coli-xdna-engine-submit");
        let compile = std::process::Command::new("cc")
            .arg("-std=c11")
            .arg("-D_GNU_SOURCE")
            .arg("-O2")
            .arg("-Wall")
            .arg("-Wextra")
            .arg("-I")
            .arg(root.join("c"))
            .arg(root.join("c/backend_xdna.c"))
            .arg(root.join("c/xdna/engine_submit_check.c"))
            .arg("-o")
            .arg(&bin)
            .status()
            .expect("cc");
        assert!(compile.success(), "engine submit checker failed to compile");

        let helper = root.join("c/xdna/libcoli_xdna.so");
        let artifacts = root.join("c/xdna/kernels");
        let output = std::process::Command::new(&bin)
            .arg(&helper)
            .arg(&artifacts)
            .env("COLI_XDNA", "1")
            .output()
            .expect("engine submit checker");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "engine submit did not reach the NPU\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
        assert!(stdout.contains("engine=kimi_k3 submit=1"), "{stdout}");
        assert!(stdout.contains("reason=COMPLETED"), "{stdout}");

        let hooks = [
            (
                "c/colibri.c",
                "coli_xdna_engine_qualified_submit(\"colibri\"",
            ),
            ("c/glm53.c", "coli_xdna_engine_qualified_submit(\"glm53\""),
            (
                "c/inkling.c",
                "coli_xdna_engine_qualified_submit(\"inkling\"",
            ),
            (
                "c/kimi_k3.c",
                "coli_xdna_engine_qualified_submit(\"kimi_k3\"",
            ),
            (
                "c/deepseek_v4.c",
                "coli_xdna_engine_qualified_submit(\"deepseek_v4\"",
            ),
            (
                "c/deepseek_v41.c",
                "coli_xdna_engine_qualified_submit(\"deepseek-v41\"",
            ),
            ("c/mimo.c", "coli_xdna_engine_qualified_submit(\"mimo\""),
            ("c/qwen36.c", "coli_xdna_engine_qualified_submit(\"qwen36\""),
            ("c/qwen38.c", "coli_xdna_engine_qualified_submit(\"qwen38\""),
            ("c/olmoe.c", "coli_xdna_engine_qualified_submit(\"olmoe\""),
        ];
        for (rel, needle) in hooks {
            let text = std::fs::read_to_string(root.join(rel)).expect(rel);
            assert!(
                text.contains(needle),
                "{rel} does not call the lane ({needle})"
            );
        }
    }
}
