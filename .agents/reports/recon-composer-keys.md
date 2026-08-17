# Recon: native GPUI composer keyboard shortcuts

Read-only inventory. No product edits.

## Where the composer lives

One widget owns every native field, including the chat prompt:

- `/home/hunter/Projects/surmount/colibri/crates/colibri-native/src/text_input.rs`
  - `TextInput` (single-line, adapted from GPUI 0.2 `examples/input.rs`)
  - `bind_text_input_keys` (the only `KeyBinding` table in this crate)
  - `key_context("TextInput")` + `on_action` listeners
- `/home/hunter/Projects/surmount/colibri/crates/colibri-native/src/main.rs`
  - `DesktopApp.chat_input` at line 253 (`TextInput::new(..., "Message colibrì…")`)
  - Same type for model, temperature, max tokens, grammar, and install fields
  - Startup: `bind_text_input_keys(cx)` at line 5166
  - Send is **mouse only**: `#btn-send` `on_mouse_up` → `send_chat` (around 4541–4572)
- `/home/hunter/Projects/surmount/colibri/crates/colibri-native/src/wizard.rs`
  - Pure step machine. No input widget, no keys.

There is no second composer. No app-level Enter / Submit action.

## What works today (named keys + path)

Registered actions: `Backspace`, `Delete`, `Left`, `Right`, `SelectAll`, `Home`, `End`, `Paste`, `Cut`, `Copy`.

| Key | Binding | Handler | Behavior |
|-----|---------|---------|----------|
| Backspace | `"backspace"` | `backspace` | If empty selection, extend to previous **grapheme** (`previous_boundary`), then delete |
| Delete | `"delete"` | `delete` | Same, next grapheme (`next_boundary`) |
| Left / Right | `"left"` / `"right"` | `left` / `right` | Collapse to start/end if a selection exists; else move one grapheme |
| Home | `"home"` | `home` | Cursor to **offset 0** (whole-buffer start) |
| End | `"end"` | `end` | Cursor to **`content.len()`** (whole-buffer end) |
| Ctrl+A / Cmd+A | `"ctrl-a"` / `"cmd-a"` | `select_all` | Select entire buffer |
| Ctrl+C / Cmd+C | `"ctrl-c"` / `"cmd-c"` | `copy` | Clipboard if selection nonempty |
| Ctrl+X / Cmd+X | `"ctrl-x"` / `"cmd-x"` | `cut` | Copy + delete selection |
| Ctrl+V / Cmd+V | `"ctrl-v"` / `"cmd-v"` | `paste` | Insert clipboard; **newlines flattened to spaces** |

Typing / IME go through `EntityInputHandler` + `window.handle_input`, not `KeyBinding`.

Mouse: click places caret; drag selects; **Shift+click** extends (`on_mouse_down` + `event.modifiers.shift`). That is the only shift-select path.

Home/End are document start/end. The field is single-line, so that is also line start/end. There is no line-vs-document distinction and no `"ctrl-home"` / `"ctrl-end"` binding.

## What is missing / broken vs a normal Linux prompt

GPUI matches bindings with exact modifiers. Unbound chords do nothing (they do not fall through to `"backspace"` / `"left"`).

| Key | Status |
|-----|--------|
| **Ctrl+Backspace** | Missing. No `DeleteWordBack` (or similar). Chord is unbound. |
| **Ctrl+Delete** | Missing. No delete-next-word. |
| **Ctrl+Left / Ctrl+Right** | Missing. No word jump. |
| **Ctrl+Home / Ctrl+End** | Missing. On this single-line field they should alias Home/End. |
| **Shift+Left / Shift+Right** | Missing. Official GPUI example has `SelectLeft` / `SelectRight` + `"shift-left"` / `"shift-right"`. Colibri dropped those. |
| **Shift+Home / Shift+End** | Missing. No select-to-start/end. |
| **Ctrl+Shift+Left / Right** | Missing. No word select. |
| **Ctrl+Shift+Home / End** | Missing. Would equal Shift+Home/End while single-line. |
| Ctrl+A / C / X / V | Works (see above). |
| Delete / Backspace (no mods) | Works (grapheme). |
| Home / End (no mods) | Works as buffer start/end. Fine for single-line. Wrong if the field later becomes multiline (would still jump the whole document). |

Also not asked, but adjacent:

- **Enter to send** is not bound. Web sends on Enter. Native hint is `"Type a message, then Send"` (`i18n.rs`); send is the button only.
- **Shift+Enter newline** cannot exist until the field is multiline. Paste already strips `\n`.
- No Undo / Redo.

## Web SPA (original composer)

`/home/hunter/Projects/surmount/colibri/web/src/App.tsx` (around 367–371): a real `<textarea>` (`components/ui/textarea.tsx`).

Custom key handling is only:

- Enter (not Shift, not IME composing) → `preventDefault` + `send()`

Everything else (Ctrl+Backspace, Home/End, Ctrl+Home/End, word jump, shift-select, clipboard) is the **browser / OS textarea**. That is why the web prompt feels like a normal Linux field.

Web copy: `"Enter to send · Shift+Enter for newline"` (`web/src/i18n/en.ts`). Native is single-line and does not implement that contract.

## Existing tests

Only paint tests in `text_input.rs` (`selection_tests`):

- `doge_selection_fill_is_pure_eight_or_opaque_primary`
- `mint_selection_uses_soft_alpha_path`

No tests for caret, selection, word bounds, or `KeyBinding` tables. Movement helpers (`previous_boundary`, `next_boundary`, `move_to`, `select_to`) are private and Window-tied on the action methods.

## Smallest product path (do not implement here)

Stay in `text_input.rs`. Do not invent a second widget. Wizard and rail fields get the same chords for free.

1. **Word offsets** (pure, `unicode-segmentation` is already a crate dep). Add `previous_word_boundary` / `next_word_boundary` (skip whitespace, then skip the word). Keep grapheme helpers for Left/Right/Backspace/Delete.
2. **Actions** (add to `actions!(colibri_text_input, [...])`):
   - Restore example: `SelectLeft`, `SelectRight`
   - New: `MoveWordLeft`, `MoveWordRight`, `SelectWordLeft`, `SelectWordRight`, `DeleteWordBack`, `DeleteWordForward`, `SelectHome`, `SelectEnd`
   - Optional aliases: `DocumentHome` / `DocumentEnd` that call the same `move_to(0)` / `move_to(len)` so Ctrl+Home/End work
3. **Handlers**: word move/select/delete reuse `move_to` / `select_to` / `replace_text_in_range`. Delete-word: if selection nonempty, delete selection (same as Backspace); else select to word bound then delete.
4. **Bindings** in `bind_text_input_keys` (Linux first; keep cmd-* mirrors where they already exist):

```
shift-left / shift-right
shift-home / shift-end
ctrl-left / ctrl-right          (+ cmd-left / cmd-right if you want mac word jump)
ctrl-shift-left / ctrl-shift-right
ctrl-backspace
ctrl-delete
ctrl-home / ctrl-end
ctrl-shift-home / ctrl-shift-end
```

5. **Render**: one `.on_action` per new handler next to the existing listeners.
6. **Home/End**: leave as buffer start/end. Do not split line vs document until multiline exists.
7. Tests: extract or `pub(crate)` the offset helpers so they do not need a `Window`. Do not require a GPUI test harness for the first green.

Leave Enter-to-send and multiline as a later slice (hint + `send_chat` wiring in `main.rs`).

## TDD contracts to add (suggested names)

Put them in `text_input.rs` `#[cfg(test)]` (or a sibling `text_input` test module). Red on helpers before binding.

- `grapheme_left_from_middle_of_ascii_word`
- `grapheme_left_skips_combining_cluster` (e.g. `e` + combining accent)
- `home_moves_to_buffer_start`
- `end_moves_to_buffer_end`
- `select_home_keeps_anchor_at_cursor` (Shift+Home)
- `select_end_extends_to_buffer_end` (Shift+End)
- `ctrl_home_aliases_buffer_start` (same offset as Home while single-line)
- `word_left_from_after_space_lands_on_word_start` (`"hello world|"` → `"hello |world"`)
- `word_left_from_mid_word_lands_on_that_word_start` (`"hello wo|rld"` → `"hello |world"`)
- `word_right_from_start_skips_to_next_word`
- `ctrl_backspace_deletes_previous_word_and_leading_spaces` (`"hello world|"` → `"hello "`)
- `ctrl_backspace_with_selection_deletes_selection_only`
- `ctrl_delete_deletes_next_word`
- `select_word_left_extends_selection` (Ctrl+Shift+Left)
- `shift_left_extends_one_grapheme` (the GPUI-example gap)
- `paste_flattens_newlines` (existing paste contract; guard so word work does not regress)

Binding table (optional, string-level): `bind_text_input_keys_registers_ctrl_backspace` if you expose the keystroke list; otherwise skip and trust `KeyBinding::new` plus handler tests.

## Bottom line

The chat prompt is a lean single-line GPUI field. Character delete/move, Home/End-as-buffer, and Ctrl+A/C/X/V work. Word chords, Shift+arrow/Home/End, and Ctrl+Home/End are unbound. The web composer feels normal because it is a real `<textarea>`. Smallest fix is more actions + word helpers in `text_input.rs` only.
