# impl: post-setup hide first-run Setup CTA

## Problem

After the wizard finished (`status` "Setup complete", `first_run_done = true`), empty chat still showed:

- "First time here? Open Setup for a short guided path."
- Green center **Setup** button

Rail **Setup** (re-entry) is fine; the center first-run CTA was wrong.

## Fix

Gate empty-chat first-run CTA on the same flag the wizard uses (`first_run_done`).

| Piece | Behavior |
|-------|----------|
| `show_first_run_setup_cta(first_run_done)` | `true` only when `!first_run_done` |
| Hero `hero.setupHint` + `hero-btn-setup` | Rendered only when that helper is true |
| After setup, engine down, no model | `hero.nextNeedModel`: pick/install in Tools or rail, then Start engine |
| After setup, engine down, model ok | `hero.nextStartEngine`: Start engine in left rail, then chat |
| After setup, engine live | No extra next-step line |
| Rail Setup | Unchanged (intentional re-open) |

Pure helpers live next to other chrome paint helpers in `main.rs`:

- `show_first_run_setup_cta`
- `hero_next_step` / `hero_next_step_key` / `HeroNextStep`
- `hero_model_ok` (dir + `config.json`)

i18n: `hero.nextNeedModel`, `hero.nextStartEngine` (en + it).

## Tests (TDD)

- `show_first_run_setup_cta_false_when_first_run_done`
- `hero_next_step_after_setup_guides_engine_not_setup` (copy must not re-pitch first-run Setup)
- `hero_model_ok_requires_config_json`

## Also

- `host.rs` test `engine_session_start_preflight_rejects_empty_store`: stopped using `unwrap_err` (needs `Debug` on `EngineSession`); match on `Err` instead so the suite compiles.

## Verify

```text
cargo fmt -p colibri-native
cargo clippy -p colibri-native --all-targets -- -D warnings   # exit 0
cargo test -p colibri-native                                  # 217 passed
```

## Files

- `crates/colibri-native/src/main.rs` — helpers, `chat_hero` gate, chrome tests
- `crates/colibri-native/src/i18n.rs` — next-step strings
- `crates/colibri-native/src/host.rs` — preflight test compile fix
