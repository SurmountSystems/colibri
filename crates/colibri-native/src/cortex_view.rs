//! Cortex camera and projected expert cells for the native Brain view.
//!
//! Orbit and zoom follow `web/src/lib/cortex.ts` (`initialCamera`, `rotate`, `zoom`, `project`).
//! Key steps follow `web/src/BrainWorkspace.tsx`. `region` is the open layer (`None` is the overview).

// Sign: +yaw is cortex.ts rotate(+x) (right / drag-right); +pitch is rotate(+y) (down / drag-down).

/// Pitch clamp from `cortex.ts` `rotate` (`-1.1..=1.1`).
const PITCH_MIN: f32 = -1.1;
/// Pitch clamp from `cortex.ts` `rotate` (`-1.1..=1.1`).
const PITCH_MAX: f32 = 1.1;
/// Distance clamp from `cortex.ts` `zoom` (`0.25..=4.4`).
const DISTANCE_MIN: f32 = 0.25;
/// Distance clamp from `cortex.ts` `zoom` (`0.25..=4.4`).
const DISTANCE_MAX: f32 = 4.4;

/// `initialCamera.yaw` in `cortex.ts`.
const INITIAL_YAW: f32 = 0.14;
/// `initialCamera.pitch` in `cortex.ts`.
const INITIAL_PITCH: f32 = 0.87;
/// `initialCamera.distance` in `cortex.ts`.
const INITIAL_DISTANCE: f32 = 3.65;

/// Arrow step passed to `rotate` in `BrainWorkspace.tsx`.
const KEY_ORBIT_STEP: f32 = 0.1;
/// Plus / minus step passed to `zoom` in `BrainWorkspace.tsx` (`zoom(-0.15)` / `zoom(0.15)`).
const KEY_ZOOM_STEP: f32 = 0.15;
/// Pointer delta scale passed to `rotate` in `BrainWorkspace.tsx` (`dx * 0.006`).
const DRAG_PER_PIXEL: f32 = 0.006;

/// Cells closer than this are dropped, matching `cortex.ts` `if (depth < .06) return null`.
const DEPTH_MIN: f32 = 0.06;

/// Canonical paint width. `project_cells` uses the `cortex.ts` focal rule against this frame.
pub const CORTEX_VIEW_W: f32 = 800.0;
/// Canonical paint height. The shell can scale [`ProjectedCell`] x/y from this frame.
pub const CORTEX_VIEW_H: f32 = 600.0;

/// Orbit camera for the Brain cortex.
///
/// `yaw`, `pitch`, and `distance` match `cortex.ts`. `region` is the open layer.
/// `None` is the overview. Enter sets it when a cell is focused. Escape clears it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CortexCamera {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub region: Option<u32>,
}

impl Default for CortexCamera {
    fn default() -> Self {
        Self {
            yaw: INITIAL_YAW,
            pitch: clamp_pitch(INITIAL_PITCH),
            distance: clamp_distance(INITIAL_DISTANCE),
            region: None,
        }
    }
}

impl CortexCamera {
    fn orbit(self, yaw_delta: f32, pitch_delta: f32) -> Self {
        Self {
            yaw: self.yaw + yaw_delta,
            pitch: clamp_pitch(self.pitch + pitch_delta),
            ..self
        }
    }

    fn zoom(self, delta: f32) -> Self {
        Self {
            distance: clamp_distance(self.distance + delta),
            ..self
        }
    }
}

fn clamp_pitch(pitch: f32) -> f32 {
    pitch.clamp(PITCH_MIN, PITCH_MAX)
}

fn clamp_distance(distance: f32) -> f32 {
    distance.clamp(DISTANCE_MIN, DISTANCE_MAX)
}

/// Apply one cortex key.
///
/// `"left"` / `"right"` / `"up"` / `"down"` orbit. `"+"` zooms in and `"-"` zooms out.
/// `"enter"` opens layer 0 when no region is open (the overview's focused cell, same role as
/// `enterTopic(topics[0])` in `BrainWorkspace.tsx`). `"escape"` clears `region`.
///
/// When a specific cell is focused, use [`cortex_enter_focused`] so Enter opens that layer.
pub fn cortex_apply_key(cam: CortexCamera, key: &str) -> CortexCamera {
    match key {
        "left" => cam.orbit(-KEY_ORBIT_STEP, 0.0),
        "right" => cam.orbit(KEY_ORBIT_STEP, 0.0),
        "up" => cam.orbit(0.0, -KEY_ORBIT_STEP),
        "down" => cam.orbit(0.0, KEY_ORBIT_STEP),
        "+" => cam.zoom(-KEY_ZOOM_STEP),
        "-" => cam.zoom(KEY_ZOOM_STEP),
        "enter" => {
            if cam.region.is_none() {
                cortex_enter_focused(cam, 0)
            } else {
                cam
            }
        }
        "escape" => {
            let mut next = cam;
            next.region = None;
            next
        }
        _ => cam,
    }
}

/// Open `layer` because that cell is focused.
///
/// Web Enter on a focused expert calls `enterExpert`; on the overview it opens the first region.
/// Here the focused cell's layer becomes `region`, and [`project_cells`] keeps only that layer.
pub fn cortex_enter_focused(cam: CortexCamera, layer: u32) -> CortexCamera {
    let mut next = cam;
    next.region = Some(layer);
    next
}

/// Drag orbit. `dx` and `dy` are pixels, scaled by `0.006` as in `BrainWorkspace.tsx`.
///
/// Positive `dx` increases yaw. Positive `dy` increases pitch. See the sign comment above.
pub fn cortex_apply_drag(cam: CortexCamera, dx: f32, dy: f32) -> CortexCamera {
    cam.orbit(dx * DRAG_PER_PIXEL, dy * DRAG_PER_PIXEL)
}

/// Zoom by adding `delta` to distance, matching `cortex.ts` `zoom(delta)`.
///
/// Negative delta moves closer (web `"+"` / zoom-in button). Positive delta moves away.
pub fn cortex_apply_zoom(cam: CortexCamera, delta: f32) -> CortexCamera {
    cam.zoom(delta)
}

/// One expert cell in the canonical view frame ([`CORTEX_VIEW_W`] × [`CORTEX_VIEW_H`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProjectedCell {
    pub x: f32,
    pub y: f32,
    pub rgb: u32,
    pub layer: u16,
    pub expert: u16,
}

/// Project `(layer, expert, rgb)` triples with the `cortex.ts` camera.
///
/// When `cam.region` is `Some(layer)`, only that layer is returned (Enter opened it).
/// Escape (`region == None`) returns every layer. Order follows the input after that filter.
/// A cell with depth under `0.06` is omitted, matching `cortex.ts`.
pub fn project_cells(cam: &CortexCamera, cells: &[(u16, u16, u32)]) -> Vec<ProjectedCell> {
    let max_layer = cells.iter().map(|cell| cell.0).max().unwrap_or(0);
    let max_expert = cells.iter().map(|cell| cell.1).max().unwrap_or(0);
    let (cx, cy, focal) = view_frame();
    let cos_y = cam.yaw.cos();
    let sin_y = cam.yaw.sin();
    let cos_p = cam.pitch.cos();
    let sin_p = cam.pitch.sin();

    let mut out = Vec::new();
    for &(layer, expert, rgb) in cells {
        if let Some(region) = cam.region {
            if u32::from(layer) != region {
                continue;
            }
        }
        let (wx, wy, wz) = cell_world(layer, expert, max_layer, max_expert);
        // Camera look-at is the origin, matching `initialCamera` x/y/z = 0 in cortex.ts.
        let x = wx * cos_y + wz * sin_y;
        let z0 = -wx * sin_y + wz * cos_y;
        let y = wy * cos_p - z0 * sin_p;
        let z = wy * sin_p + z0 * cos_p;
        let depth = cam.distance - z;
        if depth < DEPTH_MIN {
            continue;
        }
        let scale = focal / depth;
        out.push(ProjectedCell {
            x: cx + x * scale,
            y: cy + y * scale,
            rgb,
            layer,
            expert,
        });
    }
    out
}

/// Focal length and center, copied from `cortex.ts` `drawBrain`.
fn view_frame() -> (f32, f32, f32) {
    let focal = (CORTEX_VIEW_W * 1.08).min(CORTEX_VIEW_H * 1.2);
    let cx = CORTEX_VIEW_W * 0.5;
    let cy = CORTEX_VIEW_H * 0.43;
    (cx, cy, focal)
}

/// Lay the layer × expert grid on a shallow dome around the origin.
///
/// Bounds come from the full input, so opening a region hides other layers
/// without rescaling the one that stays.
fn cell_world(layer: u16, expert: u16, max_layer: u16, max_expert: u16) -> (f32, f32, f32) {
    let x = (f32::from(expert) - f32::from(max_expert) * 0.5) * 0.08;
    let y = (f32::from(layer) - f32::from(max_layer) * 0.5) * 0.06;
    let z = 0.04 * (1.0 - (x / 0.9).clamp(-1.0, 1.0).powi(2));
    (x, y, z)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<(u16, u16, u32)> {
        vec![
            (0, 0, 0x111111),
            (0, 4, 0x222222),
            (1, 1, 0x333333),
            (3, 2, 0x444444),
        ]
    }

    fn cell_named(cells: &[ProjectedCell], layer: u16, expert: u16) -> ProjectedCell {
        cells
            .iter()
            .copied()
            .find(|cell| cell.layer == layer && cell.expert == expert)
            .unwrap_or_else(|| panic!("missing layer {layer} expert {expert}"))
    }

    fn x_spread(cells: &[ProjectedCell]) -> f32 {
        let min_x = cells
            .iter()
            .map(|cell| cell.x)
            .fold(f32::INFINITY, f32::min);
        let max_x = cells
            .iter()
            .map(|cell| cell.x)
            .fold(f32::NEG_INFINITY, f32::max);
        max_x - min_x
    }

    #[test]
    fn default_matches_cortex_initial_camera() {
        let cam = CortexCamera::default();
        assert_eq!(cam.yaw, 0.14);
        assert_eq!(cam.pitch, 0.87);
        assert_eq!(cam.distance, 3.65);
        assert_eq!(cam.region, None);
        assert!((-1.1..=1.1).contains(&cam.pitch));
        assert!((0.25..=4.4).contains(&cam.distance));
    }

    #[test]
    fn arrow_keys_match_cortex_rotate_signs() {
        let cam = CortexCamera::default();
        assert_eq!(cortex_apply_key(cam, "left").yaw, cam.yaw - 0.1);
        assert_eq!(cortex_apply_key(cam, "right").yaw, cam.yaw + 0.1);
        assert_eq!(cortex_apply_key(cam, "up").pitch, cam.pitch - 0.1);
        assert_eq!(cortex_apply_key(cam, "down").pitch, cam.pitch + 0.1);
        assert_eq!(cortex_apply_key(cam, "left").pitch, cam.pitch);
        assert_eq!(cortex_apply_key(cam, "up").yaw, cam.yaw);
    }

    #[test]
    fn pitch_and_distance_clamp_to_cortex_ranges() {
        let high = CortexCamera {
            pitch: 1.05,
            distance: 4.3,
            ..Default::default()
        };
        let down = cortex_apply_key(high, "down");
        let farther = cortex_apply_key(high, "-");
        assert_eq!(down.pitch, 1.1);
        assert_eq!(farther.distance, 4.4);

        let low = CortexCamera {
            pitch: -1.05,
            distance: 0.3,
            ..Default::default()
        };
        assert_eq!(cortex_apply_key(low, "up").pitch, -1.1);
        assert_eq!(cortex_apply_key(low, "+").distance, 0.25);
        assert_eq!(cortex_apply_zoom(low, -1.0).distance, 0.25);
        assert_eq!(cortex_apply_zoom(high, 1.0).distance, 4.4);
    }

    #[test]
    fn plus_zooms_in_and_minus_zooms_out() {
        let cam = CortexCamera::default();
        assert_eq!(cortex_apply_key(cam, "+"), cortex_apply_zoom(cam, -0.15));
        assert_eq!(cortex_apply_key(cam, "-"), cortex_apply_zoom(cam, 0.15));
        assert!(cortex_apply_key(cam, "+").distance < cam.distance);
        assert!(cortex_apply_key(cam, "-").distance > cam.distance);
    }

    #[test]
    fn drag_uses_pointer_scale_and_cortex_signs() {
        let cam = CortexCamera::default();
        let dragged = cortex_apply_drag(cam, 10.0, -5.0);
        assert_eq!(dragged.yaw, cam.yaw + 10.0 * 0.006);
        assert_eq!(dragged.pitch, cam.pitch - 5.0 * 0.006);
        assert_eq!(dragged.distance, cam.distance);
        assert_eq!(dragged.region, None);
        let clamped = cortex_apply_drag(CortexCamera { pitch: 1.1, ..cam }, 0.0, 50.0);
        assert_eq!(clamped.pitch, 1.1);
    }

    #[test]
    fn unknown_key_is_ignored() {
        let cam = CortexCamera::default();
        assert_eq!(cortex_apply_key(cam, "ArrowRight"), cam);
        assert_eq!(cortex_apply_key(cam, ""), cam);
    }

    #[test]
    fn rotating_changes_projected_x() {
        let cells = sample();
        let cam = CortexCamera::default();
        let turned = cortex_apply_key(cam, "right");
        let dragged = cortex_apply_drag(cam, 80.0, 0.0);
        let before = cell_named(&project_cells(&cam, &cells), 0, 4).x;
        let after_key = cell_named(&project_cells(&turned, &cells), 0, 4).x;
        let after_drag = cell_named(&project_cells(&dragged, &cells), 0, 4).x;
        assert_ne!(before, after_key);
        assert_ne!(before, after_drag);
    }

    #[test]
    fn pitching_changes_projected_y() {
        let cells = sample();
        let cam = CortexCamera::default();
        let up = cortex_apply_key(cam, "up");
        let before = cell_named(&project_cells(&cam, &cells), 3, 2).y;
        let after = cell_named(&project_cells(&up, &cells), 3, 2).y;
        assert_ne!(before, after);
    }

    #[test]
    fn zooming_changes_spread() {
        let cells = sample();
        let cam = CortexCamera::default();
        let closer = cortex_apply_zoom(cam, -1.0);
        let farther = cortex_apply_zoom(cam, 0.8);
        let spread_near = x_spread(&project_cells(&closer, &cells));
        let spread_mid = x_spread(&project_cells(&cam, &cells));
        let spread_far = x_spread(&project_cells(&farther, &cells));
        assert!(spread_near > spread_mid);
        assert!(spread_mid > spread_far);
        let keyed = x_spread(&project_cells(&cortex_apply_key(cam, "+"), &cells));
        assert!(keyed > spread_mid);
    }

    #[test]
    fn enter_and_escape_change_which_layers_project() {
        let cells = sample();
        let cam = CortexCamera::default();
        let opened = cortex_apply_key(cam, "enter");
        assert_eq!(opened.region, Some(0));
        assert_eq!(opened.yaw, cam.yaw);
        let overview = project_cells(&cam, &cells);
        let inside = project_cells(&opened, &cells);
        assert!(overview.len() > inside.len());
        assert!(inside.iter().all(|cell| cell.layer == 0));
        assert_eq!(inside.len(), 2);
        assert_eq!(cell_named(&inside, 0, 4).rgb, 0x222222);

        let focused = cortex_enter_focused(cam, 3);
        let only_layer = project_cells(&focused, &cells);
        assert_eq!(only_layer.len(), 1);
        assert_eq!(only_layer[0].layer, 3);
        assert_eq!(only_layer[0].expert, 2);

        let back = cortex_apply_key(focused, "escape");
        assert_eq!(back.region, None);
        assert_eq!(project_cells(&back, &cells).len(), overview.len());
        assert_eq!(cortex_apply_key(opened, "enter").region, Some(0));
    }

    #[test]
    fn missing_region_projects_nothing() {
        let cells = sample();
        let cam = cortex_enter_focused(CortexCamera::default(), 9);
        assert!(project_cells(&cam, &cells).is_empty());
    }

    #[test]
    fn empty_input_projects_nothing() {
        assert!(project_cells(&CortexCamera::default(), &[]).is_empty());
    }
}
