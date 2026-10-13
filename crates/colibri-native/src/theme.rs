//! Theme palettes for colibri-native (GPUI).
//!
//! Two themes ship:
//! - **DOGE** (default): pure 8-color emissive palette only
//!   ([0001_DOGE.md](https://github.com/SurmountSystems/specs/blob/main/0001_DOGE.md),
//!   accessed: 2026-08-11). Colors are exactly `#FF0000` `#00FF00` `#0000FF`
//!   `#00FFFF` `#FF00FF` `#FFFF00` `#000000` `#FFFFFF`.
//! - **Mint**: SPA-family tokens matching `web/src/index.css` (soft midtones).
//!
//! Layout density targets the same product family as the React dashboard, not
//! pixel-perfect spacing. Color fields are `0xRRGGBB` for `gpui::rgb`.
//!
//! Prefer `palette(ThemeId)` (or `DesktopApp::palette`) over the mint-only
//! module consts below. Those consts remain as mint aliases for gradual
//! migration and unit tests that pin mint phase hues.
//!
//! Full paint migration + prefs/Tools wire still in progress; keep symbols live
//! for unit tests and upcoming call sites.
#![allow(dead_code)]

// ---------------------------------------------------------------------------
// Theme id
// ---------------------------------------------------------------------------

/// Which visual theme the shell paints with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ThemeId {
    /// Pure 8-color DOGE palette (default).
    #[default]
    Doge,
    /// Soft mint tokens from the web SPA.
    Mint,
}

impl ThemeId {
    /// Parse a preference / env string (`"doge"` / `"mint"`). Unknown → Doge.
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "mint" => ThemeId::Mint,
            _ => ThemeId::Doge,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ThemeId::Doge => "doge",
            ThemeId::Mint => "mint",
        }
    }

    /// Map a prefs theme id onto paint tokens.
    pub fn from_pref(pref: crate::prefs::ThemePref) -> Self {
        match pref {
            crate::prefs::ThemePref::Doge => ThemeId::Doge,
            crate::prefs::ThemePref::Mint => ThemeId::Mint,
        }
    }

    /// Map paint theme onto prefs for save.
    pub fn to_pref(self) -> crate::prefs::ThemePref {
        match self {
            ThemeId::Doge => crate::prefs::ThemePref::Doge,
            ThemeId::Mint => crate::prefs::ThemePref::Mint,
        }
    }
}

// ---------------------------------------------------------------------------
// DOGE pure colors (spec Clause 4)
// ---------------------------------------------------------------------------

/// DOGE red `#FF0000`.
pub const DOGE_RED: u32 = 0xFF_00_00;
/// DOGE green `#00FF00`.
pub const DOGE_GREEN: u32 = 0x00_FF_00;
/// DOGE blue `#0000FF`.
pub const DOGE_BLUE: u32 = 0x00_00_FF;
/// DOGE cyan `#00FFFF`.
pub const DOGE_CYAN: u32 = 0x00_FF_FF;
/// DOGE magenta `#FF00FF`.
pub const DOGE_MAGENTA: u32 = 0xFF_00_FF;
/// DOGE yellow `#FFFF00`.
pub const DOGE_YELLOW: u32 = 0xFF_FF_00;
/// DOGE black `#000000`.
pub const DOGE_BLACK: u32 = 0x00_00_00;
/// DOGE white `#FFFFFF`.
pub const DOGE_WHITE: u32 = 0xFF_FF_FF;

/// The eight pure DOGE colors (spec Clause 4 order: RGBCMYKW).
pub const DOGE_EIGHT: [u32; 8] = [
    DOGE_RED,
    DOGE_GREEN,
    DOGE_BLUE,
    DOGE_CYAN,
    DOGE_MAGENTA,
    DOGE_YELLOW,
    DOGE_BLACK,
    DOGE_WHITE,
];

// ---------------------------------------------------------------------------
// Palette
// ---------------------------------------------------------------------------

/// All paint roles used by the native shell (rail, chat, brain, profiling).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemePalette {
    pub bg: u32,
    pub panel: u32,
    pub border: u32,
    pub text: u32,
    pub muted: u32,
    pub label: u32,
    pub primary: u32,
    pub primary_fg: u32,
    pub primary_wash: u32,
    pub primary_border: u32,
    pub secondary: u32,
    pub chip: u32,
    pub warn: u32,
    pub danger: u32,
    pub ok: u32,
    pub speed: u32,
    pub tier_disk: u32,
    pub user_body: u32,
    pub assist_body: u32,
    // Profiling phase hues
    pub phase_io_wait: u32,
    pub phase_matmul: u32,
    pub phase_attention: u32,
    pub phase_lm_head: u32,
    pub phase_other: u32,
    // Badge chip fills / borders (mint soft washes; DOGE pure)
    pub badge_live_bg: u32,
    pub badge_live_border: u32,
    pub badge_speed_bg: u32,
    pub badge_speed_border: u32,
    pub badge_warn_bg: u32,
    pub badge_warn_border: u32,
}

impl ThemePalette {
    /// Every role color in this palette (for DOGE purity tests).
    pub fn all_role_colors(self) -> [u32; 30] {
        [
            self.bg,
            self.panel,
            self.border,
            self.text,
            self.muted,
            self.label,
            self.primary,
            self.primary_fg,
            self.primary_wash,
            self.primary_border,
            self.secondary,
            self.chip,
            self.warn,
            self.danger,
            self.ok,
            self.speed,
            self.tier_disk,
            self.user_body,
            self.assist_body,
            self.phase_io_wait,
            self.phase_matmul,
            self.phase_attention,
            self.phase_lm_head,
            self.phase_other,
            self.badge_live_bg,
            self.badge_live_border,
            self.badge_speed_bg,
            self.badge_speed_border,
            self.badge_warn_bg,
            self.badge_warn_border,
        ]
    }
}

/// Resolve the palette for a theme id.
pub fn palette(id: ThemeId) -> ThemePalette {
    match id {
        ThemeId::Doge => doge_palette(),
        ThemeId::Mint => mint_palette(),
    }
}

/// Mint SPA-family tokens (`web/src/index.css`).
pub fn mint_palette() -> ThemePalette {
    ThemePalette {
        bg: 0x08_0b_0d,
        panel: 0x0d_12_15,
        border: 0x20_2a_2f,
        text: 0xe9_ef_f0,
        muted: 0x96_a4_a9,
        label: 0x66_74_7a,
        primary: 0x4e_d6_a5,
        primary_fg: 0x05_21_18,
        primary_wash: 0x0a_2a_20,
        primary_border: 0x1a_4a_3a,
        secondary: 0x15_1c_20,
        chip: 0x1a_22_26,
        warn: 0xe6_aa_3c,
        danger: 0xff_76_6f,
        ok: 0x4e_d6_a5,
        speed: 0x5a_9b_d8,
        tier_disk: 0x3a_47_50,
        user_body: 0xae_b9_bd,
        assist_body: 0xd8_e0_e2,
        phase_io_wait: 0x39_87_e5,
        phase_matmul: 0x19_9e_70,
        phase_attention: 0xc9_85_00,
        phase_lm_head: 0x00_83_00,
        phase_other: 0x90_85_e9,
        badge_live_bg: 0x12_35_2a,
        badge_live_border: 0x1a_4a_3a,
        badge_speed_bg: 0x15_26_36,
        badge_speed_border: 0x2a_4a_66,
        badge_warn_bg: 0x2a_24_10,
        badge_warn_border: 0x5a_48_20,
    }
}

/// DOGE pure 8-color mapping (default theme).
///
/// Role map (plan + DOGE Clause 4):
/// | Role | Color |
/// |------|-------|
/// | bg / panel / secondary / wash | Black |
/// | text / assist / user body | White |
/// | muted / label | Cyan |
/// | primary / ok | Green |
/// | primary_fg (on green) | Black |
/// | primary_border / chip | Blue (chip) or Green (border) |
/// | danger | Red |
/// | warn | Yellow |
/// | speed (info accent) | Magenta |
/// | tier_disk / borders | Blue / White |
/// | phase colors | Blue, Green, Yellow, Magenta, Cyan |
pub fn doge_palette() -> ThemePalette {
    ThemePalette {
        bg: DOGE_BLACK,
        panel: DOGE_BLACK,
        border: DOGE_WHITE,
        text: DOGE_WHITE,
        muted: DOGE_CYAN,
        label: DOGE_CYAN,
        primary: DOGE_GREEN,
        primary_fg: DOGE_BLACK,
        primary_wash: DOGE_BLACK,
        primary_border: DOGE_GREEN,
        secondary: DOGE_BLACK,
        chip: DOGE_BLUE,
        warn: DOGE_YELLOW,
        danger: DOGE_RED,
        ok: DOGE_GREEN,
        speed: DOGE_MAGENTA,
        tier_disk: DOGE_BLUE,
        user_body: DOGE_WHITE,
        assist_body: DOGE_WHITE,
        // Fixed DOGE phase map (no soft midtones)
        phase_io_wait: DOGE_BLUE,
        phase_matmul: DOGE_GREEN,
        phase_attention: DOGE_YELLOW,
        phase_lm_head: DOGE_MAGENTA,
        phase_other: DOGE_CYAN,
        badge_live_bg: DOGE_BLACK,
        badge_live_border: DOGE_GREEN,
        badge_speed_bg: DOGE_BLACK,
        badge_speed_border: DOGE_MAGENTA,
        badge_warn_bg: DOGE_BLACK,
        badge_warn_border: DOGE_YELLOW,
    }
}

// ---------------------------------------------------------------------------
// Layout (theme-independent)
// ---------------------------------------------------------------------------

/// Left rail width (web sidebar ~292px).
pub const RAIL_WIDTH: f32 = 292.0;
/// Hero content max width.
pub const HERO_MAX_W: f32 = 680.0;
/// Setup wizard card max width (centered in main column).
pub const WIZARD_MAX_W: f32 = 720.0;

/// Product chrome corner radius in pixels.
///
/// DOGE (and shell chrome generally) uses **hard edges only**: 1px solid
/// borders, radius 0. No pills, no soft cards. Pass to GPUI `.rounded(px(...))`
/// only when an explicit radius is required; otherwise omit `.rounded_*`.
pub const CORNER_RADIUS: f32 = 0.0;

// ---- Spacing tokens (comfortable density; hard edges unchanged) -----------

/// Outer padding on the left rail (px).
pub const RAIL_PAD: f32 = 20.0;
/// Vertical gap between major rail blocks (brand, engine, inference, footer).
pub const RAIL_SECTION_GAP: f32 = 16.0;
/// Inner padding for rail cards (engine path, inference, install).
pub const RAIL_CARD_PAD: f32 = 16.0;
/// Gap between children inside a rail card.
pub const RAIL_CARD_GAP: f32 = 12.0;
/// Compact control button horizontal pad (Start/Stop, chip buttons).
pub const BTN_PAD_X: f32 = 12.0;
/// Compact control button vertical pad.
pub const BTN_PAD_Y: f32 = 8.0;
/// Wizard stage outer margin (empty space around the card).
pub const WIZARD_STAGE_PAD: f32 = 32.0;
/// Wizard card inner padding.
pub const WIZARD_CARD_PAD: f32 = 32.0;
/// Vertical rhythm between wizard step label, title, body, and step content.
pub const WIZARD_CONTENT_GAP: f32 = 16.0;
/// Max height of the supported-models list viewport (wizard + Tools). Scrolls when taller.
/// ~5–6 compact rows at rail density so the list does not dominate the dialog.
pub const WIZARD_CATALOG_LIST_MAX_H: f32 = 168.0;
/// Max height of scanned registry entry lists inside wizard Model / Doctor steps.
pub const WIZARD_REGISTRY_LIST_MAX_H: f32 = 112.0;
/// Approximate catalog / registry row height (text-xs + py_1 + border + gap).
pub const WIZARD_LIST_ROW_H: f32 = 28.0;
/// Text field horizontal pad (so glyphs are not jammed to the border).
pub const FIELD_PAD_X: f32 = 12.0;
/// Text field vertical pad.
pub const FIELD_PAD_Y: f32 = 8.0;
/// Minimum text field height (content + pad + border).
pub const FIELD_MIN_H: f32 = 36.0;

// ---------------------------------------------------------------------------
// Mint-only module consts (compat aliases for gradual migration)
// ---------------------------------------------------------------------------
// Prefer `palette(id).field` in new paint code. These match `mint_palette()`.

/// Near-black teal-tinted page background (`--background: #080b0d`). Mint only.
pub const BG: u32 = 0x08_0b_0d;
/// Card / panel surface (`--card: #0d1215`). Mint only.
pub const PANEL: u32 = 0x0d_12_15;
/// Borders (`--border: #202a2f`). Mint only.
pub const BORDER: u32 = 0x20_2a_2f;
/// Primary text (`--foreground: #e9eff0`). Mint only.
pub const TEXT: u32 = 0xe9_ef_f0;
/// Muted secondary text (`--muted-foreground: #96a4a9`). Mint only.
pub const MUTED: u32 = 0x96_a4_a9;
/// Section label gray. Mint only.
pub const LABEL: u32 = 0x66_74_7a;
/// Brand mint primary. Mint only.
pub const PRIMARY: u32 = 0x4e_d6_a5;
/// Primary on dark. Mint only.
pub const PRIMARY_FG: u32 = 0x05_21_18;
/// Soft mint wash. Mint only.
pub const PRIMARY_WASH: u32 = 0x0a_2a_20;
/// Dim mint border wash. Mint only.
pub const PRIMARY_BORDER: u32 = 0x1a_4a_3a;
/// Secondary surface. Mint only.
pub const SECONDARY: u32 = 0x15_1c_20;
/// Inactive button / chip fill. Mint only.
pub const CHIP: u32 = 0x1a_22_26;
/// Warning amber. Mint only.
pub const WARN: u32 = 0xe6_aa_3c;
/// Destructive / stop. Mint only.
pub const DANGER: u32 = 0xff_76_6f;
/// Success / ok. Mint only.
pub const OK: u32 = 0x4e_d6_a5;
/// Speed / RAM tier blue. Mint only.
pub const SPEED: u32 = 0x5a_9b_d8;
/// Disk tier bar. Mint only.
pub const TIER_DISK: u32 = 0x3a_47_50;
/// Message body muted user tone. Mint only.
pub const USER_BODY: u32 = 0xae_b9_bd;
/// Assistant body. Mint only.
pub const ASSIST_BODY: u32 = 0xd8_e0_e2;

// ---- Profiling phase colors (web Profiling.tsx PHASES) — mint ------------

/// I/O wait phase. Mint only.
pub const PHASE_IO_WAIT: u32 = 0x39_87_e5;
/// Expert matmul phase. Mint only.
pub const PHASE_MATMUL: u32 = 0x19_9e_70;
/// Attention phase. Mint only.
pub const PHASE_ATTENTION: u32 = 0xc9_85_00;
/// LM head phase. Mint only.
pub const PHASE_LM_HEAD: u32 = 0x00_83_00;
/// Other residual phase. Mint only.
pub const PHASE_OTHER: u32 = 0x90_85_e9;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_id_default_is_doge() {
        assert_eq!(ThemeId::default(), ThemeId::Doge);
    }

    #[test]
    fn theme_id_parse() {
        assert_eq!(ThemeId::parse("doge"), ThemeId::Doge);
        assert_eq!(ThemeId::parse("DOGE"), ThemeId::Doge);
        assert_eq!(ThemeId::parse("mint"), ThemeId::Mint);
        assert_eq!(ThemeId::parse("Mint"), ThemeId::Mint);
        assert_eq!(ThemeId::parse("nope"), ThemeId::Doge);
        assert_eq!(ThemeId::parse(""), ThemeId::Doge);
    }

    #[test]
    fn doge_palette_every_color_is_one_of_eight() {
        let p = doge_palette();
        for c in p.all_role_colors() {
            assert!(
                DOGE_EIGHT.contains(&c),
                "DOGE palette color 0x{c:06X} is not in the pure eight"
            );
        }
    }

    #[test]
    fn doge_role_map_basics() {
        let p = doge_palette();
        assert_eq!(p.bg, DOGE_BLACK);
        assert_eq!(p.panel, DOGE_BLACK);
        assert_eq!(p.text, DOGE_WHITE);
        assert_eq!(p.muted, DOGE_CYAN);
        assert_eq!(p.primary, DOGE_GREEN);
        assert_eq!(p.danger, DOGE_RED);
        assert_eq!(p.warn, DOGE_YELLOW);
        assert_eq!(p.speed, DOGE_MAGENTA);
        assert_eq!(p.border, DOGE_WHITE);
    }

    #[test]
    fn mint_palette_matches_legacy_consts() {
        let p = mint_palette();
        assert_eq!(p.bg, BG);
        assert_eq!(p.panel, PANEL);
        assert_eq!(p.border, BORDER);
        assert_eq!(p.text, TEXT);
        assert_eq!(p.muted, MUTED);
        assert_eq!(p.label, LABEL);
        assert_eq!(p.primary, PRIMARY);
        assert_eq!(p.primary_fg, PRIMARY_FG);
        assert_eq!(p.primary_wash, PRIMARY_WASH);
        assert_eq!(p.primary_border, PRIMARY_BORDER);
        assert_eq!(p.secondary, SECONDARY);
        assert_eq!(p.chip, CHIP);
        assert_eq!(p.warn, WARN);
        assert_eq!(p.danger, DANGER);
        assert_eq!(p.ok, OK);
        assert_eq!(p.speed, SPEED);
        assert_eq!(p.tier_disk, TIER_DISK);
        assert_eq!(p.user_body, USER_BODY);
        assert_eq!(p.assist_body, ASSIST_BODY);
        assert_eq!(p.phase_io_wait, PHASE_IO_WAIT);
        assert_eq!(p.phase_matmul, PHASE_MATMUL);
        assert_eq!(p.phase_attention, PHASE_ATTENTION);
        assert_eq!(p.phase_lm_head, PHASE_LM_HEAD);
        assert_eq!(p.phase_other, PHASE_OTHER);
    }

    #[test]
    fn palette_dispatch() {
        assert_eq!(palette(ThemeId::Doge), doge_palette());
        assert_eq!(palette(ThemeId::Mint), mint_palette());
    }

    #[test]
    fn corner_radius_is_sharp() {
        assert_eq!(CORNER_RADIUS, 0.0);
    }

    #[test]
    fn spacing_tokens_are_positive_and_ordered() {
        // Compile-time pin: tokens stay positive and comfortably ordered.
        const {
            assert!(RAIL_PAD > 0.0);
            assert!(RAIL_SECTION_GAP > 0.0);
            assert!(RAIL_CARD_PAD > 0.0);
            assert!(RAIL_CARD_GAP > 0.0);
            assert!(BTN_PAD_X > 0.0);
            assert!(BTN_PAD_Y > 0.0);
            assert!(WIZARD_STAGE_PAD > 0.0);
            assert!(WIZARD_CARD_PAD > 0.0);
            assert!(WIZARD_CONTENT_GAP > 0.0);
            assert!(WIZARD_CATALOG_LIST_MAX_H > WIZARD_LIST_ROW_H);
            assert!(WIZARD_REGISTRY_LIST_MAX_H > WIZARD_LIST_ROW_H);
            assert!(WIZARD_LIST_ROW_H > 0.0);
            // Catalog viewport holds several rows without dominating the card.
            assert!(WIZARD_CATALOG_LIST_MAX_H >= WIZARD_LIST_ROW_H * 4.0);
            assert!(WIZARD_CATALOG_LIST_MAX_H <= WIZARD_LIST_ROW_H * 8.0);
            assert!(FIELD_PAD_X > 0.0);
            assert!(FIELD_PAD_Y > 0.0);
            assert!(FIELD_MIN_H > FIELD_PAD_Y * 2.0);
            assert!(RAIL_PAD >= RAIL_CARD_PAD - 4.0);
            assert!(RAIL_SECTION_GAP >= RAIL_CARD_GAP);
            assert!(WIZARD_CARD_PAD >= RAIL_CARD_PAD);
            assert!(WIZARD_CONTENT_GAP >= RAIL_CARD_GAP);
            assert!(WIZARD_MAX_W >= HERO_MAX_W);
            assert!(RAIL_WIDTH > 200.0);
        }
    }
}
