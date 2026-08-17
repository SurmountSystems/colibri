# L3 implementer: composer keys (text_input.rs only)

You are an L3 general-purpose implementer. **No L4.** Do not spawn further agents.

Repo: `/home/hunter/Projects/surmount/colibri`

## File ownership (hard)

**ONLY edit** `crates/colibri-native/src/text_input.rs`.

Do **not** edit `main.rs` (`bind_text_input_keys` is already called). Do not edit any other file.

## Goal

Make the native single-line `TextInput` (chat prompt and every other field) work like a normal Linux field: word move/delete, Shift-select, Ctrl+Home/End aliases.

Plan: `~/.grok/sessions/%2Fhome%2Fhunter%2FProjects%2Fsurmount%2Fcolibri/019fe97a-838f-71b1-b3a3-db8c7493555f/plan.md` step 1.
Recon (do not re-walk the tree): `.agents/reports/recon-composer-keys.md`

## Recommended decisions (locked)

- Home/End stay buffer start/end (`offset 0` / `len`).
- Ctrl+Home / Ctrl+End alias the same offsets.
- Do **not** add Enter-to-send or multiline.

## What to implement

1. **Pure helpers** (so tests do not need a GPUI `Window`):
   - `previous_word_boundary(text, offset)` / `next_word_boundary(text, offset)`
   - Skip whitespace, then skip the word. Keep existing grapheme `previous_boundary` / `next_boundary` for Left/Right/Backspace/Delete.
   - `unicode-segmentation` is already a crate dep.

2. **Actions** in `actions!(colibri_text_input, [...])`:
   - Restore `SelectLeft`, `SelectRight`
   - Add `MoveWordLeft`, `MoveWordRight`, `SelectWordLeft`, `SelectWordRight`, `DeleteWordBack`, `DeleteWordForward`, `SelectHome`, `SelectEnd`
   - Ctrl+Home/End: either reuse `Home`/`End` bindings or add alias actions that call the same `move_to(0)` / `move_to(len)`
   - Ctrl+Shift+Home/End: alias `SelectHome` / `SelectEnd`

3. **Handlers**: reuse `move_to` / `select_to` / `replace_text_in_range`. Word delete: if selection nonempty, delete selection only (same as Backspace); else select to word bound then delete.

4. **Bindings** in `bind_text_input_keys` (Linux first; keep existing cmd-* mirrors and add cmd word mirrors if you already have cmd-* for clipboard):

```
shift-left / shift-right
shift-home / shift-end
ctrl-left / ctrl-right
ctrl-shift-left / ctrl-shift-right
ctrl-backspace
ctrl-delete
ctrl-home / ctrl-end
ctrl-shift-home / ctrl-shift-end
```

Optional cmd mirrors for word jump on mac: `cmd-left` / `cmd-right` / `cmd-backspace` / `cmd-delete` / `cmd-shift-left` / `cmd-shift-right` / `cmd-home` / `cmd-end`. Do not steal Super/global shortcuts.

5. **Render**: one `.on_action` per new handler next to existing listeners.

6. **Paste**: keep flattening newlines to spaces. Guard with `paste_flattens_newlines`.

## TDD (required)

Write tests **first**. Run them. Observe **red**. Then product edit. Do not rewrite expectations to finish green.

Put tests in `text_input.rs` `#[cfg(test)]`. Extract or `pub(crate)` offset helpers so they do not need a Window. You may add a tiny test-only buffer (content + range + apply helper) to exercise delete/select without GPUI.

**Required test names:**

- `word_left_from_after_space_lands_on_word_start`
- `word_left_from_mid_word_lands_on_that_word_start`
- `ctrl_backspace_deletes_previous_word_and_leading_spaces`
- `ctrl_backspace_with_selection_deletes_selection_only`
- `ctrl_delete_deletes_next_word`
- `home_moves_to_buffer_start`
- `end_moves_to_buffer_end`
- `ctrl_home_aliases_buffer_start`
- `select_home_keeps_anchor_at_cursor`
- `shift_left_extends_one_grapheme`
- `paste_flattens_newlines`

You may add siblings (`word_right_*`, `select_end_*`, `ctrl_end_*`) but do not weaken the named list.

## Verify

```
cargo test -p colibri-native --lib text_input
cargo fmt -p colibri-native
cargo clippy -p colibri-native --all-targets -- -D warnings
```

Log command + exit code in the report.

## Report

Write `/home/hunter/Projects/surmount/colibri/.agents/reports/l3-composer-keys.md` with:

- files changed
- RED: command, test name, fail reason **before** product edit
- GREEN: same command passed
- fmt / clippy / test commands + exit codes
- what landed

Never git add / commit / push. No implement-run hex in product source.
