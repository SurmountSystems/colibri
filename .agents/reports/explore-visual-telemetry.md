# Explore: visual / telemetry surfaces (colibri-sys planning)

**Scope:** map what visual and metrics data exist today and how they are exposed, for a future Rust embedded + streaming API (zerocopy / rkyv duplex).
**Sources:** `docs/serve_protocol.md`, `docs/routing-telemetry.md`, `docs/api.md`, `c/telemetry.h`, `c/route_trace.h`, `c/colibri.c` mux path, `c/openai_server.py`, `web/src/*`, expert atlas tools.
**Date:** 2026-08-09.

---

## 1. Architecture (today)

```
engine (C, stdin/stdout line protocol)
    → openai_server.py (latest-snapshot state + HTTP JSON + OpenAI SSE chat)
        → web SPA polls GET endpoints; chat uses SSE token stream
```

- **No WebSocket.** Dashboard is HTTP poll + chat SSE only.
- **No push of brain/profile updates** mid-request to the UI (UI polls).
- Engine mux protocol is line-oriented text + length-prefixed `DATA` payloads (`docs/serve_protocol.md`).

**Poll cadence (web):**

| Endpoint | Component | Interval |
|----------|-----------|----------|
| `GET /health` | App runtime sidebar | 5s |
| `GET /experts` | Brain canvas | 1.5s |
| `GET /profile` | Profiling page | 2s |
| `GET /experts.json` (static) | Brain hover atlas | once on mount |

---

## 2. Engine → server wire (mux telemetry)

### Implemented and parsed by `openai_server.Engine`

| Line | When | Shape (fields) | Server state |
|------|------|----------------|--------------|
| `READY` / startup `STAT` | handshake | sentinel + rss | startup only |
| `HWINFO` | startup + end of turn (`mux_done`) | `cores ram_total_gb ram_avail_gb ngpu vram_total_gb cpu\|gpu` | `engine.hwinfo` |
| `TIERS` | startup + end of turn | `vram_n ram_n disk_n vram_gb ram_gb` | `engine.tiers` |
| `EMAP` | startup + end of turn | `rows cols hex` | `engine.emap` |
| `HITS` | **end of turn only** (not every ~6 tokens; doc is stale) | `rows cols hex_bits` | `engine.hits`, `hits_seq++` |
| `PROF` | end of turn | `wall_s prompt completion edisk ewait emm attn head n_fw` | `profile` deque (maxlen 120), `profile_seq++` |
| `DATA` | per token | `id nbytes` + raw UTF-8 | chat stream |
| `ACCEPT` | pre-prefill ok | `id prompt_tokens` | commit stream / early error path |
| `DONE` | end of turn | `id STAT completion tok/s hit% rss_gb prompt length_limited` | request done stats |
| `ERROR` | failures | `id CODE…` | request fail |

Emitters live in `c/telemetry.h` (`hwinfo_emit`, `tiers_emit`, `emap_emit`, `hits_emit`) and `colibri.c` `mux_done` / mux startup (`PROF`, `DONE`).

### Documented in `serve_protocol.md` but **not** in current gateway path

| Line | Doc intent | Reality |
|------|------------|---------|
| `PERF` | turn PROFILO deltas | Engine emits **`PROF`** (different field order / names); server listens for `PROF` only |
| `ENTROPY` | per-sparse-layer routing entropy | Not parsed; no engine emit found on mux path |
| `GPUS` | per-device VRAM + expert counts | Not parsed; not in `/health` or `/experts` |
| `REPIN` | live re-pin swap events | stderr / `REPIN` env only; not mux→server |
| `TOPK` | sampled token candidates | `SERVE_TOPK` mentioned in docs; not wired through gateway |
| SSE `data: {"colibri":{…}}` | extra frame before `[DONE]` | **Not implemented** in `openai_server.py` |
| `/experts` fields `gpus, entropy, repin` | doc claim | Actual JSON: `rows, cols, map, hits, seq` only |

**Forward-compat rule (real):** servers must ignore unknown line kinds; today’s Python dispatcher **raises** on unknown kinds (`invalid engine response`), so it is stricter than the doc.

### Off-HTTP routing history (placement / tools, not dashboard)

From `route_trace.h` / `docs/routing-telemetry.md`:

- **`.coli_usage`**: sparse text triples `layer expert count` + headers `-1 dims`, `-2 version engine_hash`.
- **`ROUTE_TRACE=`**: lines `call row layer id:gate …` (measurement; disables device router).
- **IKU1** dense binary (inkling history only).

Useful for offline atlas / PIN, not live UI.

---

## 3. HTTP JSON shapes the UI actually consumes

### `GET /health` (auth for full body when key set)

```json
{
  "status": "ok",
  "scheduler": {
    "active": 0, "queued": 0, "capacity": 1, "max_queue": 8,
    "queue_timeout_seconds": 300,
    "admitted": 0, "completed": 0, "rejected": 0, "timed_out": 0, "cancelled": 0
  },
  "kv_slots": 1,
  "tiers": { "vram": N, "ram": N, "disk": N, "vram_gb": f, "ram_gb": f },
  "hwinfo": {
    "cores": N, "ram_total_gb": f, "ram_avail_gb": f,
    "gpus": N, "vram_total_gb": f, "cpu": "…", "gpu": "…"
  }
}
```

**Visual use (`App.tsx`):** hardware rows; scheduler tiles; **tier bar** (VRAM/RAM/disk expert counts + GB for VRAM/RAM).

### `GET /experts`

```json
{ "rows": R, "cols": C, "map": "<2 hex chars × R×C>", "hits": "<2 hex chars × ceil(R×C/8)>", "seq": N }
```

**EMAP byte packing (per expert, row-major sparse layers + optional MTP):**

```
byte = (tier << 6) | heat
tier: 0=disk, 1=RAM, 2=VRAM
heat: 0..63 = floor(log2(usage))+1 style (while u>>=1)
```

**HITS:** bit per expert since last emit; **emit clears** `g_ehit`. Hex is little-endian bit packing: expert index `i` → byte `i>>3`, bit `i&7`.

**Visual use (`Brain.tsx`):** cortex grid; tier color + heat brightness; pulse on hit bits when `seq` changes. Tier totals counted from `map`.

### `GET /profile`

```json
{
  "seq": N,
  "turns": [{
    "wall_s", "prompt_tokens", "completion_tokens",
    "expert_disk_s", "expert_wait_s", "expert_matmul_s",
    "attention_s", "lm_head_s", "forwards"
  }, …]  // up to 120
}
```

**Visual use (`Profiling.tsx`):** tok/s tiles, phase share bars, stacked phase chart, throughput chart, table. Client derives `other_s = wall - (ewait+emm+attn+head)`.

### Chat SSE / usage (OpenAI path)

- Token text deltas; final `usage` when `stream_options.include_usage`.
- Headers: `x-colibri-queue-wait-ms`, `x-request-id`.
- **Client-only metrics:** live token flash, tok/s, TTFT, session prompt+completion totals (`App.tsx` from stream clock + usage). Not engine telemetry.

### Static `experts.json` (atlas, offline publish)

Web format from `tools/expert_atlas/analyze.py --web`:

```json
{
  "categories": ["poetry", "code", …],
  "experts": {
    "3:42": {
      "affinity": { "code": 0.4, "poetry": 0.1, … },
      "entropy": 1.23,
      "top": "code",
      "label": "specialist: code" | "generalist"
    }
  }
}
```

**Visual use:** Brain hover only. README still shows a 3-D **Atlas galaxy** page; **no `Atlas` view remains under `web/src`** (docs/marketing ahead of tree).

---

## 4. Visual inventory (what a human sees)

| Surface | Data | Source |
|---------|------|--------|
| Expert tier bar | counts + VRAM/RAM GB | `TIERS` → `/health.tiers` |
| HW panel | CPU name, cores, RAM total/free, GPU count/VRAM | `HWINFO` → `/health.hwinfo` |
| Scheduler tiles | active/capacity, queue, completed, failures | Python scheduler |
| Brain cortex | R×C grid (e.g. GLM ~76×256), tier color, heat brightness | `EMAP` → `/experts.map` |
| Brain flash | recent routed experts pulse | `HITS` + `seq` |
| Brain tooltip | layer (UI maps row→layer heuristically `row+3` / MTP 78), expert, tier, heat | map + optional atlas key `layer:expert` |
| Atlas specialty | topic affinity / entropy / specialist label | static `experts.json` |
| Depth role text | early…final cortex bands when no atlas | pure client heuristic |
| Profiling | phase stack, tok/s, disk service vs wait | `PROF` → `/profile` |
| Chat badges | tok/s, TTFT, prompt→completion, queue wait, slot | SSE/client + headers |
| Session totals | prompt/completion sum | client aggregate |

**Not live in UI today:** per-GPU expert residency (`GPUS`), turn routing entropy series (`ENTROPY`), REPIN events, TOPK candidates, mid-decode HITS cadence, full gate-level route trace.

---

## 5. Binary sizes (zerocopy / rkyv planning)

Typical GLM-scale sparse map: **rows ≈ 76, cols = 256 → N = 19 456 experts**.

| Frame | Logical payload | Notes |
|-------|-----------------|-------|
| Expert map | `N × u8` packed tier\|heat | hex today ≈ 2N chars over wire |
| Hits bitmap | `ceil(N/8)` bytes | hex today ≈ N/4 chars |
| Tiers | 3×u32 + 2×f32 | tiny |
| HWINFO | ints + floats + short strings | tiny; names variable |
| PROF turn | 4×f32 + 2×u32 + 3×f32 + u64 | ~40 B |
| Profile window | ×120 | ~5 KB |
| Atlas static | sparse map keyed by `(layer,eid)` | optional sidecar, not per-token |
| Route trace | variable top-k ids+gates per moe call | high rate if ever streamed |

**rkyv-friendly fixed layouts (recommended cores):**

```text
ExpertCell   { tier: u2-in-u8, heat: u6 }  // already one byte
HitsBitmap   { rows: u16, cols: u16, bits: [u8; ceil(rows*cols/8)] }
TiersSnap    { vram_n, ram_n, disk_n: u32; vram_gb, ram_gb: f32 }
HwSnap       { cores: u16; ngpu: u8; ram_total, ram_avail, vram_total: f32; cpu/gpu: short fixed or len-prefixed }
ProfTurn     { wall, edisk, ewait, emm, attn, head: f32; prompt, completion: u32; forwards: u64 }
SchedulerSnap{ active, queued, capacity, max_queue, admitted, completed, rejected, timed_out, cancelled: u32; queue_timeout_s: f32 }
TokenDelta   { req_id: u64; utf8: [u8] }   // or keep UTF-8 chunked
DoneStats    { req_id; completion; prompt; tok_s; hit_pct; rss_gb; length_limited }
```

Prefer **raw bytes for map/hits** (drop hex). Sequence numbers (`hits_seq`, `profile_seq`) for delta detection.

---

## 6. Doc vs code gaps (plan against code)

1. **HITS cadence:** docs “~every 6 tokens”; code emits HITS only in `mux_done` (full turn).
2. **`PERF` vs `PROF`** naming and field set diverge.
3. **SSE `colibri` extension** and **`/experts` entropy/gpus/repin** are aspirational.
4. **Atlas 3-D page** is README-only; only Brain + static JSON remain.
5. Gateway **fails closed** on unknown engine lines (stricter than protocol doc).

---

## 7. Recommended duplex stream message taxonomy (Rust API)

Design a **single binary duplex stream** (or two channels: control vs telemetry) with tagged envelopes. Duplex: client commands + server events.

### Server → client (telemetry / stream)

| Tag | Priority | Payload | Cadence |
|-----|----------|---------|---------|
| `Hello` | once | protocol version, model id, kv_slots, engine name | connect |
| `Hwinfo` | low | `HwSnap` | startup + slow (turn end / 5s) |
| `Tiers` | mid | `TiersSnap` | turn end / on pin change |
| `ExpertMap` | mid | rows, cols, `[u8; N]` | turn end / pin change |
| `ExpertHits` | high | rows, cols, bitmap + `seq` | **desired:** every K tokens; **today:** turn end |
| `ProfTurn` | mid | `ProfTurn` + `seq` | turn end |
| `Token` | high | req_id, utf8 bytes | per decode |
| `Accept` | mid | req_id, prompt_tokens | after validate |
| `Done` | mid | `DoneStats` | turn end |
| `Scheduler` | low | `SchedulerSnap` | on change / poll |
| `Error` | high | req_id, code, message | as needed |
| `Repin` *(optional)* | mid | layer, eid, old_tier, gpu | if REPIN enabled |
| `Gpus` *(optional)* | low | per-device used/total/experts | CUDA builds |
| `LayerEntropy` *(optional)* | low | `f32[rows]` | turn end if computed |
| `TopK` *(optional)* | mid | logprob + token text | if enabled |
| `AtlasBlob` *(optional)* | once | rkyv of static affinity map or URI | connect / publish |

### Client → server (control)

| Tag | Payload |
|-----|---------|
| `Submit` | req_id, slot, max_tokens, temp, top_p, prompt utf8, optional grammar/audio |
| `Stop` / `Cancel` | req_id |
| `Subscribe` | bitset of telemetry interests (map, hits, prof, hw, scheduler…) |
| `SetHitInterval` | tokens between `ExpertHits` (if engine supports) |
| `Ping` | keepalive |

### Framing notes for colibri-sys

1. **Envelope:** `u32 le length | u16 tag | u16 flags | body` (or rkyv root enum).
2. **Large maps:** send full `ExpertMap` on change; send only `ExpertHits` + `seq` for animation (matches Brain).
3. **Replace poll:** one subscription replaces `/health` + `/experts` + `/profile` polling; keep REST for OpenAI clients.
4. **Keep OpenAI SSE** as a separate compatibility face; do not force brain telemetry through OpenAI frames.
5. **Atlas** stays cold-start sidecar (static file or `AtlasBlob`); do not mix into hot path.
6. **Route_trace / .coli_usage** remain offline / placement tools unless a debug subscription is explicitly added (high volume).

---

## 8. File map (absolute)

| Path | Role |
|------|------|
| `/home/hunter/Projects/surmount/colibri/docs/serve_protocol.md` | mux protocol + aspirational HTTP extensions |
| `/home/hunter/Projects/surmount/colibri/docs/routing-telemetry.md` | `.coli_usage` / `ROUTE_TRACE` |
| `/home/hunter/Projects/surmount/colibri/docs/api.md` | serve/web feature description |
| `/home/hunter/Projects/surmount/colibri/c/telemetry.h` | HWINFO/TIERS/EMAP/HITS emit |
| `/home/hunter/Projects/surmount/colibri/c/route_trace.h` | usage history + trace stream |
| `/home/hunter/Projects/surmount/colibri/c/colibri.c` | mux_done PROF/DONE/telemetry |
| `/home/hunter/Projects/surmount/colibri/c/openai_server.py` | Engine dispatcher, `/health` `/experts` `/profile` |
| `/home/hunter/Projects/surmount/colibri/web/src/Brain.tsx` | cortex + hits pulse + atlas hover |
| `/home/hunter/Projects/surmount/colibri/web/src/Profiling.tsx` | PROF charts |
| `/home/hunter/Projects/surmount/colibri/web/src/App.tsx` | health/tiers/chat client metrics |
| `/home/hunter/Projects/surmount/colibri/web/src/lib/api.ts` | TS types + fetch helpers |
| `/home/hunter/Projects/surmount/colibri/c/tools/expert_atlas/analyze.py` | `experts.json` web shape |

---

## 9. Bottom line for colibri-sys

**Live visual stack is thin and already nearly fixed-width binary:**

1. **Cortex:** `u8[N]` map + bit hits + monotic seq.
2. **Memory bar:** five numbers (tier counts + two GB).
3. **Host:** small hw snap.
4. **Timing:** one PROF struct per turn (ring of 120).
5. **Scheduler + token stream:** separate from placement maps.

Ship a duplex telemetry channel that **binary-encodes what `/experts` + `/health` + `/profile` already carry**, drop hex, optionally restore mid-stream hits and the doc-only GPUS/ENTROPY/REPIN tags later. Treat atlas as offline static. Do not plan around the unimplemented SSE `colibri` bag without re-reading this tree.
