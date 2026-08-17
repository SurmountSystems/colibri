# Review 3/3: colibri-native determinate progress

Date: 2026-08-11
Scope: `progress.rs`, install/generate UI (`main.rs` / `host.rs`), `colibri-sys` install progress bytes, impl reports
Mode: read-only

## Summary

Pure math in `progress.rs` is solid and well-tested. Host helpers correctly prefer bytes over files and force 100% on phase `"done"`. UI strips are wired under install and chat composer.

The product path still fails the “determinate” promise in the **default install** case (`prefer_cli: true` emits no byte/file counters for the whole download). Done-frame 100% is set then cleared in the same drain, so it never paints. Inspect/register phases regress the bar from 100% back to 0%. Generate percent is max-tokens based, so short completions sit near 0% until Done (which also never paints).

## Findings (severity + file:line)

### High

1. **Default install is silent / stuck at 0% for the whole download**
   - `host.rs:2100-2106` — `install_options_for_ui` sets `prefer_cli: true`.
   - `install.rs:428-430` — CLI path downloads with **no** mid-download `on_progress` callbacks.
   - `install.rs:417-425` — only a single `download` event with all counters `None`.
   - Effect: UI start view (`main.rs:1155-1158`) stays **“Downloading... 0% · Calculating...”** for minutes; strip never moves until `inspect` / `done`.
   - Report `impl-install-generate-progress.md` acknowledges prefer-cli is coarse, but default UI still ships that path. This is the main “install without totals silent” hole.

2. **Done 100% frame never paints (generate and install)**
   - Generate: `main.rs:1004-1010` sets `progress_view_generate_done()`, then same function `main.rs:1047-1053` clears `gen_progress` when `still == false`.
   - Install: `main.rs:1219-1221` sets 100% Done, then `main.rs:1248-1253` clears `install_progress` in the same `drain_*` call.
   - `cx.notify()` does not yield a paint before clear. Impl report claims “Done → 100% briefly, then hide”; code never holds the frame across a poll tick.
   - Severity high for claimed UX contract; medium if “brief” was never product-critical.

### Medium

3. **Post-download phase regresses percent to 0%**
   - `host.rs:2066-2083` — only phase `"done"` forces 100%; `inspect` / `register` use `install_progress` with all counters `None` (`install.rs:462-469`, `486-493`).
   - Hub path can reach files/bytes complete (`install.rs:591-598` → 100%), then next event is inspect → **“Checking files... 0% · Calculating...”**. Visibly wrong bar jump.

4. **Generate percent wrong vs real completion (max-token denominator)**
   - `progress.rs:104-108`, wired at `main.rs:988-991` / `host.rs:2087-2093`.
   - Percent = `generated / max_output`. Default max is 4096 (`host.rs:926`). EOS/stop at 80 tokens ≈ 2%. Bar barely moves; ETA assumes full max remaining.
   - Not a math bug, but misleading determinate UI for chat. No alternate “indeterminate after unknown end” mode.

5. **Hub byte progress is file-granularity only (large single shard stuck at 0%)**
   - `install.rs:570-588` — `bytes_done` updates only **before** each `download_file`, after previous file finishes.
   - One multi-GB shard: strip stays 0% until that file completes, then jumps. Documented as out of slice; still a real ETA/percent edge when hub path is used.

6. **Tok/s for generate ETA includes TTFT**
   - `main.rs:978-991` — rate = `live_token_count / stream_start.elapsed()`. Prefill wait depresses early rate and inflates ETA; first tokens show wild “about N hour(s) left” until steady decode.
   - Related dead arithmetic: `main.rs:978-984` builds `elapsed` then discards it (`let _ = elapsed` with `* 0.0` term). Confusing leftover, not functional.

### Low

7. **`format_eta` minute/hour flooring understates**
   - `progress.rs:82-90` — 90s → “about 1 min left”; 7199s → “about 1 hour left”. Acceptable copy coarseness; no test for near-boundary understatement.

8. **`eta_secs` with tiny positive rate can claim near-`u64::MAX` left**
   - `progress.rs:50-66` — capped, no panic; UI can show absurd multi-year ETAs if rate is a single file after a long stall. Rare.

9. **Install cancel leaves stale strip until terminal event**
   - `main.rs:1166-1176` — status “Cancelling...” but `install_progress` unchanged until Error/Done clears it. Minor.

10. **Progress strip line omits current file; status line has it**
    - `main.rs:1210-1216` appends file to `install_status` only; `progress_strip_el` (`main.rs:1269-1308`) paints `view.line()` without file. Dual sources of truth; not wrong, slightly inconsistent chrome.

### Informational / not bugs

11. **Math module is live, not dead**
    - `progress.rs` exports are used via `host.rs` and `main.rs`. No `#![allow(dead_code)]` on progress (impl-progress-widget note is obsolete; install-generate report correctly dropped it).

12. **UI wire present**
    - Install strip: `main.rs:2474-2479` (`#install-progress`).
    - Generate strip: `main.rs:3382-3388` (`#generate-progress`).
    - Hide when idle: `Option` cleared on terminal drain. Wire is complete; behavior gaps are data + drain timing, not missing elements.

13. **Hub path does fill bytes when sizes known**
    - `install.rs:547-598`, `filter_entries_with_sizes` + test `install.rs:772-784`. Works when `prefer_cli` is false or CLI unavailable.

14. **`percent_done` / overflow / prefer-bytes**
    - Covered by tests in `progress.rs:182-410`. No review issues on pure percent math.

## Dead code / noise

| Item | Location | Note |
|------|----------|------|
| Discarded `elapsed` in gen drain | `main.rs:978-984` | Dead local; leftover from tok/s experiment |
| Immediate clear of Done views | `main.rs:1010+1053`, `1221+1253` | Assigned then unused for paint |
| `InstallProgress.message` in native UI | sys fills; host uses phase label only | Not dead in sys; unused by native chrome |

No unused public symbols found in `progress.rs` after install-generate wire-up.

## Cross-check vs impl reports

| Claim | Verdict |
|-------|---------|
| Pure math 34 tests, green | OK (tests present) |
| Generate strip under composer | Wired |
| Install strip above status | Wired |
| Hub bytes when list_tree sizes | Implemented |
| Prefer-cli coarse / no byte stream | True; **default UI still prefer-cli** |
| Done → 100% briefly then hide | **False in practice** (same-tick clear) |
| “Module is live” | True |

## Suggested fix order (for implementer; not done here)

1. Prefer hub path for determinate UI, or parse/proxy CLI progress; at minimum emit synthetic file progress if possible.
2. On terminal drain: keep 100% for one poll (~40–80 ms) or clear only after next timer tick so Done paints.
3. Hold last non-zero percent across `inspect`/`register` (or force 100% for those phases).
4. Generate: consider ETA from recent window rate (exclude TTFT); optional indeterminate label when completion is open-ended.
5. Delete dead `elapsed` block in `drain_gen`.

## Files reviewed

- `/home/hunter/Projects/surmount/colibri/crates/colibri-native/src/progress.rs`
- `/home/hunter/Projects/surmount/colibri/crates/colibri-native/src/host.rs` (progress helpers ~2020–2098, tests ~3169–3250)
- `/home/hunter/Projects/surmount/colibri/crates/colibri-native/src/main.rs` (gen/install state, drain, `progress_strip_el`, UI attach)
- `/home/hunter/Projects/surmount/colibri/crates/colibri-sys/src/model/install.rs` (`InstallProgress`, CLI vs hub, byte fill)
- `.agents/reports/impl-progress-widget.md`
- `.agents/reports/impl-install-generate-progress.md`
