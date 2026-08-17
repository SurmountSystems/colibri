# impl: install hf-hub per-file retry + exponential backoff

**Date:** 2026-08-11
**Repo:** `/home/hunter/Projects/surmount/colibri`
**Scope:** `crates/colibri-sys` install feature (`model/install.rs`) only.

---

## Root cause class

Operator install failure on large multi-shard repo
`mastouri/GLM-5.2-colibri-int4-g64-with-int8-mtp`:

```
Install error: install error: hf-hub: download_file out-00059.safetensors: HTTP request error: error decoding response body
```

**Class:** transient HTTP **body/decode** failure mid-shard (reqwest “error decoding response body”), not a permanent Hub 404/auth.

hf-hub 1.x has internal request retries (`retry.rs`), but
`is_transient_reqwest_error` **explicitly treats** `is_body()` / `is_decode()` as
**non-transient**. So this exact error class never retried inside the crate.
Product install had no per-file outer retry either: one flaky shard aborted the
whole job.

---

## Retry policy

| Knob | Default | Env override |
|------|---------|--------------|
| Total attempts per file (incl. first) | **6** | `COLIBRI_INSTALL_MAX_ATTEMPTS` |
| Or extra retries after first | (alt) | `COLIBRI_INSTALL_MAX_RETRIES` → attempts = n+1 |
| Initial backoff | **1s** | `COLIBRI_INSTALL_INITIAL_BACKOFF_MS` |
| Max single delay | **60s** | `COLIBRI_INSTALL_MAX_BACKOFF_MS` |

**Schedule (pure, no jitter):** after failed attempt *k*, wait
`min(max_backoff, initial * 2^(k-1))`.

Defaults: 1s → 2s → 4s → 8s → 16s → 32s (next would cap at 60s).

**Jitter (runtime only):** full jitter in **[50%, 100%]** of the scheduled delay
so concurrent installs do not lock-step. Unit tests assert the pure schedule.

**What is transient (retry):**

- HTTP request / body decode / incomplete message / connection reset / timeout
- 5xx, 408, 425, 429 / RateLimited
- Cache lock timeout, selected IO kinds, Xet download source messages that look transport-ish
- `HFError::MalformedResponse`

**What is permanent (no retry spin):**

- Entry/repo/revision/bucket not found, AuthRequired, Forbidden, Conflict
- Invalid parameter, cache disabled, local cache miss
- Install cancelled / paused

**Exhausted error (plain English):**

```
hf-hub download_file {file} failed after {N} attempts: {last_error}
```

---

## Code paths

| Item | Location |
|------|----------|
| Policy + backoff + classifier + `retry_transient` | `crates/colibri-sys/src/model/install.rs` (after `local_file_is_complete`) |
| Per-file `download_file` retry loop | `download_via_hf_hub` |
| `list_tree` light retry (same policy) | same function, before file loop |
| Progress “Retrying download of …” | `on_progress` in retry `on_retry` callback (does not invent %/ETA) |
| Resume skip of complete files | unchanged `local_file_is_complete` |
| Interruptible backoff sleep | `sleep_interruptible` (~100ms steps + cancel/pause check) |

Prefer-cli `hf download` path **unchanged** (this slice is hf-hub fallback).

---

## Tests (red→green contracts)

Unit tests under `model::install::tests` (no live network):

| Test | Contract |
|------|----------|
| `backoff_schedule_grows_exponentially_and_caps` | delays grow 1,2,4,… and cap at max |
| `backoff_respects_custom_initial_and_cap` | custom initial/cap |
| `default_retry_policy_matches_constants` | defaults in 5–8 attempt band |
| `transient_classifier_matches_operator_body_decode` | operator body-decode string is transient |
| `permanent_classifier_rejects_auth_and_not_found` | 404/auth/cancel permanent |
| `typed_hf_error_permanent_variants` | typed HFError permanent + Other body-decode |
| `retry_wrapper_succeeds_after_transient_failures` | mock: fail twice → success |
| `retry_wrapper_stops_immediately_on_permanent_error` | one attempt only |
| `retry_wrapper_exhausts_and_returns_last_error` | N attempts then last err |
| `exhausted_error_message_is_plain_english` | exact exhausted wording |

Implementation landed with the contracts; filters re-run green.

---

## Verify commands

| Command | Exit |
|---------|------|
| `cargo fmt -p colibri-sys` | 0 |
| `cargo clippy -p colibri-sys --all-targets --features install -- -D warnings` | 0 |
| `cargo test -p colibri-sys --features install --lib model::install::tests` | 0 (31 passed, 1 ignored) |
| `cargo test -p colibri-sys --features install --lib` | 0 (182 passed, 1 ignored) |

---

## Residual

- **Pause during backoff:** sleep is interruptible; next attempt sees cancel/pause. Pause mid-`download_file` still only cooperative between files (hf-hub blocks until send returns), same as before.
- **Prefer-cli path:** no product-level retry wrapper on `hf download` CLI (HF CLI may have its own). Only hf-hub path gained outer retries.
- **Partial shard cleanup:** failed attempt may leave a short/incomplete file; resume still re-downloads when size ≠ expected (no new cleanup).
- **Live multi-shard soak:** not run here; `live_hf_snapshot_tiny` remains `#[ignore]`.
- **hf-hub upstream:** could file/patch upstream so body/decode counts as transient; product outer retry is the durable fix regardless.

No git commit (operator-owned).
