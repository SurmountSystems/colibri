# Phase D: inference UX (embed path)

**Date:** 2026-08-10
**Scope:** colibri-sys duplex wire + colibri-native product controls
**Not in scope:** NPU run paths; true libcolibri FFI (`open:ffi-phase-d` still deferred)

## Summary

Closed production-parity inference controls so native matches web capability for
temperature, max tokens, reasoning, multi-slot sticky chat, and GBNF grammar on
the sys duplex path.

Architecture unchanged:

```
GPUI → colibri-sys (in-process host) → ServeClient mux → C engine process
```

## Files changed

| Path | Change |
|------|--------|
| `crates/colibri-sys/src/stream/frame.rs` | `ClientFrame::Submit.grammar: Option<String>`; `PROTOCOL_VERSION = 2` |
| `crates/colibri-sys/src/engine/duplex.rs` | Map grammar (and existing slot/temp/max_tokens) into `GenerateRequest`; wire tests |
| `crates/colibri-sys/src/stream/codec.rs` | Roundtrip asserts grammar + controls |
| `crates/colibri-native/src/host.rs` | `GenerateControls`, clamp/parse helpers, sticky slot transcript helper, `generate_async` uses controls + `render_chat` / `enable_thinking`; `EngineSession::kv_slots`; env `COLIBRI_KV_SLOTS` / `KV_SLOTS` |
| `crates/colibri-native/src/main.rs` | Inference panel UI; multi-slot sticky chat; send path uses controls |
| `crates/colibri-native/docs/fidelity.md` | Rows: multi-slot, temp, max tokens, reasoning, grammar → **done** |
| `.agents/RESIDUAL.md` | Closed multi-slot, grammar-submit, inference controls |

## APIs

### colibri-sys

- `ClientFrame::Submit { …, grammar: Option<String> }`
- Duplex `handle` / `handle_with` → `GenerateRequest { grammar, cache_slot, max_tokens, temperature, top_p, … }`
- Empty / `None` grammar omits the 7th SUBMIT header length field (mux prompt-only payload)

### colibri-native host

- `GenerateControls { temperature, max_tokens, enable_thinking, cache_slot, grammar, top_p }`
- Bounds: temperature 0–2, max tokens 1–32768, slot `0..kv_slots`, top_p 0–1
- `controls_from_ui`, `parse_temperature`, `parse_max_tokens`, `parse_grammar_field`
- `switch_cache_slot_transcript` for sticky per-slot chat logs
- `EngineSession::generate_async(slot, messages, controls, tx)` (replaces bare `max_tokens`)
- `EngineSession::kv_slots()`; start uses `env_kv_slots()` (default 1)
- Reasoning is **host chat-template** (`ChatRenderOptions.enable_thinking`), not a mux SUBMIT field

### GPUI labels (plain English)

- Temperature, Max tokens, Reasoning: on/off, Session slot N of M, Grammar (GBNF, optional)

## Tests (red contract → green)

| Test | Package | Contract |
|------|---------|----------|
| `duplex_submit_forwards_slot_temp_max_tokens_and_grammar` | colibri-sys | SUBMIT header + grammar payload match ClientFrame |
| `duplex_submit_omits_grammar_field_when_none` | colibri-sys | No 7th header field when grammar absent |
| `client_frame_roundtrip` (extended) | colibri-sys | rkyv roundtrip includes grammar |
| `clamp_temperature_bounds` | colibri-native | 0–2, non-finite → default |
| `clamp_max_tokens_bounds` | colibri-native | 1–32768 |
| `clamp_cache_slot_respects_kv_slots` | colibri-native | slot ≤ kv_slots−1 |
| `parse_temperature_and_max_tokens` | colibri-native | UI string parse + clamp |
| `parse_grammar_field_empty_is_none` | colibri-native | blank → None |
| `generate_controls_clamped_clears_blank_grammar` | colibri-native | full clamp path |
| `controls_from_ui_builds_clamped` | colibri-native | form → controls |
| `switch_cache_slot_transcript_is_sticky` | colibri-native | stash/restore per slot |
| `clamp_kv_slots_bounds` | colibri-native | 1–16 |
| `generate_async_errors_when_no_session` | colibri-native | updated signature |

## Residual closed

- `open:multi-slot` → closed
- `open:grammar-submit` → closed
- Product notes for temperature / max tokens / reasoning recorded under CLOSED

Still open (unchanged): brain full atlas, live HWINFO strip, deep doctor UI, FFI Phase D, Tauri parity, OpenAI REST.

## Verify commands

```text
cargo fmt -p colibri-sys -p colibri-native
  exit 0

cargo clippy -p colibri-sys --all-targets -- -D warnings
  exit 0

cargo clippy -p colibri-native --all-targets -- -D warnings
  exit 0

cargo test -p colibri-sys --lib
  exit 0  (85 passed)

cargo test -p colibri-native
  exit 0  (40 passed)
```

## Operator note

Multi-slot needs `KV_SLOTS` / `COLIBRI_KV_SLOTS` > 1 when starting the engine (default remains 1). Prev/Next session controls appear when the live session advertises more than one slot.
