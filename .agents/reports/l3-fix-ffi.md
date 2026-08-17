# L3 report: FFI / C review-fix (Issues 1, 3, 5, 12, 13)

Workflow spawn from L2 was blocked. Slice done in the L2 thread.

## Files

- `c/colibri.c`, `c/colibri_api.h`, `c/st.h`
- `crates/colibri-sys/src/plan.rs`
- `crates/colibri-sys/src/ffi/mod.rs`, `ffi/bindings.rs`
- `crates/colibri-sys/docs/ffi-phase-d.md`

## Product

- `g_mem_avail_boot` sampled before `model_init`. Inject: `COLI_TEST_MEM_AVAIL_GB` / `COLI_TEST_MEM_AVAIL_AFTER_GB`.
- `model_release` + `st_close` on RAM refuse and destroy.
- Default-path prefill: stop check in the layer loop; leftover skip via `coli_prefill_should_run_leftover`.
- `ram_overcommit_from` matches C `atoi` (`"1foo"` is 1).

## RED

C leftover symbol missing (`coli_prefill_should_run_leftover`) until the C definition landed. Mid-range / atoi tests were written first.

## GREEN

```
cargo test -p colibri-sys --lib --features ffi -- cap_for_ram ram_overcommit coli_embed_should_stop coli_prefill_should_run_leftover glm_tiny_open_uses_preload
```

10 passed (with the log tests in the same filter). `glm_tiny` open with 64 GiB pre / 0.5 GiB leftover printed `[RAM_GB=56.3 auto] cap=64 ok`. Exit 0.

## Residual

Prefill Stop claim is default-path honest. Isolation names pre-load sample and `model_release`.
