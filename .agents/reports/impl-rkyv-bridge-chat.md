# Implement: rkyv duplex bridge + chat templates (colibri-sys)

**Repo:** `/home/hunter/Projects/surmount/colibri`
**Crate:** `crates/colibri-sys`
**Date:** 2026-08-10
**Status:** done (fmt + clippy `-D warnings` + unit tests green)

---

## What shipped

### 1. Engine rkyv bridge (`runtime` + `stream`)

| Item | Path |
|------|------|
| `EngineDuplex` | `crates/colibri-sys/src/engine/duplex.rs` |
| Progressive mux API | `ServeClient::generate_stream`, `EngineHandle::generate_stream` |
| Visual pump | `EngineHandle::pump_visual`, `EngineHandle::with_client` |

**Architecture (documented in module + user-guide §8.1):**

- App speaks `ClientFrame` / `ServerFrame` (rkyv) via `EngineDuplex`.
- Bridge translates to `ServeClient` line protocol (`SUBMIT` / `DATA` / `DONE` / visual).
- Engine stays a **C subprocess** over stdin/stdout. Not REST, not gRPC, not FFI.

**Frame handling:**

| ClientFrame | Behavior |
|-------------|----------|
| `Submit` | `GenerateRequest` → stream `Accept` / `Token` (when `Subscribe::TOKENS` or `ALL`) → `Done` + subscribed visual |
| `Stop` / `Cancel` | `ServeClient::stop_request(req_id)` |
| `Subscribe` | Update mask; emit current visual snapshot |
| `Ping` | `Pong` |

**Hello:** `EngineDuplex::hello()` fills protocol version, model id, engine basename, `kv_slots` from config.

**Unit tests (mock pipes, no real engine):** ping/pong, submit token stream, subscribe mask suppresses tokens, `generate_stream` DATA callback.

### 2. Chat templates (always-on)

| Item | Path |
|------|------|
| Module | `crates/colibri-sys/src/chat.rs` |
| API | `render_chat`, `render_chat_simple`, `ChatMessage`, `ChatRole`, `ChatRenderOptions` |

Ports text multi-turn from `c/openai_server.py`:

- GLM (+ OLMoE) → `render_chat_glm`
- Kimi K3 → length-framed `K3CHAT1`
- DeepSeek V4 → native begin/User/Assistant markers
- Inkling → TMLv0 role tokens + effort hint + content prefill

Golden unit tests for multi-turn GLM / Kimi / V4 / Inkling. Tools / Inkling audio not fully ported (host gap noted in docs).

### 3. Re-exports (`lib.rs`)

- Always: `chat::*` (`render_chat`, `ChatMessage`, …)
- `runtime`: `ServeEvent` (for stream callbacks)
- `runtime` + `stream`: `EngineDuplex`

### 4. Docs

`crates/colibri-sys/docs/user-guide.md` §8 expanded:

- §8.1 `EngineDuplex` over serve mux (not REST)
- §8.2 chat templates for native hosts

---

## Verify

```bash
cd /home/hunter/Projects/surmount/colibri
cargo fmt -p colibri-sys
cargo clippy -p colibri-sys --all-targets -- -D warnings
cargo test -p colibri-sys --lib
```

**Result:** clippy clean; **58** unit tests passed (including 4 duplex + 1 stream + 7 chat).

---

## Host usage sketch (GPUI / embed)

```rust
use colibri_sys::{
    ChatMessage, ClientFrame, EngineDuplex, EngineHandle, ModelFamily,
    render_chat_simple,
};

// after plan + EngineHandle::start_with_plan(...)
let mut duplex = EngineDuplex::new(handle, "local-model");
let _ = duplex.hello();

let prompt = render_chat_simple(
    &[
        ChatMessage::system("You are helpful."),
        ChatMessage::user("Hi"),
    ],
    ModelFamily::Glm,
)?;

duplex.handle_with(
    &ClientFrame::Submit {
        req_id: 1,
        slot: 0,
        max_tokens: 64,
        temperature: 0.8,
        top_p: 0.95,
        prompt,
    },
    |frame| {
        // ServerFrame::Token / Done / Hwinfo / …
        Ok(())
    },
)?;
```

---

## Intentional gaps (not this slice)

- No REST/HTTP server in colibri-sys
- No in-process `libcolibri` FFI
- Full tool-call + Inkling audio templates remain Python-heavy
- Grammar field still only on `GenerateRequest`, not on `ClientFrame::Submit`
- Scheduler frame still host-side (no mux source)
