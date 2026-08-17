# Host ROCm presence (read-only probe)

Date: 2026-08-11  
Host probes only; informational.

## ROCm present?

**Yes.** Stack under `/opt/rocm`.

| Item | Value |
|------|--------|
| `rocminfo` | `/opt/rocm/bin/rocminfo` |
| `hipcc` | `/opt/rocm/bin/hipcc` |
| `rocm-smi` | `/opt/rocm/bin/rocm-smi` |
| `ROCM_PATH` | `/opt/rocm` |
| `HIP_PATH` | empty (unset) |
| ROCm package version (`/opt/rocm/.info/version`) | **7.2.4** |
| HIP (`hipcc --version`) | **HIP version: 7.2.53211-9999** (AMD clang 22.0.0git) |

`/opt/rocm` top entries seen: `amdgcn`, `bin`, `hiprand`, `include`, `lib`, `libexec`, `llvm`, `share`.

## GPU / agents (`rocminfo` / `rocm-smi` / `lspci`)

| Agent | Marketing / name | Type |
|-------|------------------|------|
| Agent 1 | AMD Ryzen AI 7 PRO 350 w/ Radeon 860M | CPU |
| Agent 2 | AMD Radeon 860M Graphics (`gfx1102`) | GPU |
| Agent 3 | RyzenAI-npu6 (`aie2p`) | DSP (NPU) |

- `rocm-smi --showproductname`: Card Series **AMD Radeon 860M Graphics**, SKU **STRIXEMU**, GFX Version reported **gfx1152**, Card Model `0x1114`.
- `lspci` display: `c5:00.0` AMD/ATI **Krackan [Radeon 840M / 860M Graphics]** (rev d2).
- Platform PCI: AMD **Krackan** root complex / data fabric (APU-class host).

Note: rocminfo GPU name string is `gfx1102`; rocm-smi reports GFX **gfx1152**. Both refer to the same Radeon 860M device.

## Memory

| | |
|--|--|
| Total RAM | **89 Gi** (`free -h`) |
| Swap | 184 Gi total |

## APU / shared memory?

**Yes, looks like an APU with integrated GPU (shared system memory).**

- CPU marketing name embeds Radeon 860M.
- Display is Krackan Radeon 840M/860M iGPU, not a discrete dGPU PCI product name.
- ROCm GPU agent is that same 860M; no separate discrete AMD GPU agent listed in the probe.

No dmesg collected (not requested).
