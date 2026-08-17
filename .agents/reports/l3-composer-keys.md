# Slice report: composer keys

**Scope:** `crates/colibri-native/src/text_input.rs` only.
**Note:** Implemented in the L2 thread (workflow spawn blocked from this session).

## Contract

Word move/select/delete, Shift-select, Home/End = buffer start/end, Ctrl+Home/End alias the same. Paste flattens newlines. No Enter-to-send.

## Red

```text
cargo test -p colibri-native --bin colibri-native text_input
```

Fail: 5 tests. Word helpers returned the input offset; flatten was identity.

## Green

Same command, 14 passed, exit 0.

## Landed

- `previous_word_boundary` / `next_word_boundary` / `next_word_delete_end` via `split_word_bound_indices`.
- Actions: `SelectLeft`/`Right`, `MoveWordLeft`/`Right`, `SelectWordLeft`/`Right`, `DeleteWordBack`/`Forward`, `SelectHome`/`End`.
- Bindings: ctrl-backspace/delete, ctrl-left/right, shift-left/right, shift-home/end, ctrl-shift-left/right, ctrl-home/end, ctrl-shift-home/end.
- Home/End stay offset 0 / `len`. Did not edit `main.rs`.
