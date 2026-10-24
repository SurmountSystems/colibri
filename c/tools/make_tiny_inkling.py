#!/usr/bin/env python3
"""Build a tiny random-weight Inkling text model + oracle fixture for inkling.c.

Stage-A validation, mirroring the OLMoE ref.json flow: saves a small
InklingForCausalLM snapshot (safetensors + config.json) and a ref_inkling.json
with {prompt_ids, full_ids, tf_pred} from HF transformers, which the C engine
must reproduce token-for-token (run with bits=0 for f32-exact experts).

The tiny config exercises every architectural branch of the big model:
sliding + global layers, log-length tau scaling (n_floor set below the
sequence length), dense + sparse MLP layers, 2 shared experts, unpadded vocab.

Usage: python3 make_tiny_inkling.py <outdir> [--wide]

--wide keeps the real model's hidden size, 6144, and head dim, 128, in two layers (a
sliding one with a dense MLP, a global one with the experts): the size the Vulkan
expert batch stages its input at, and the dense chain's matrices, norms and
convolutions at the width they run at.
"""
import json
import os
import struct
import sys

import torch

try:
    from transformers import InklingForCausalLM, InklingTextConfig
except ImportError:
    sys.exit("transformers has no Inkling support: pip install -U transformers")


def pin_tiny_moe_widths(cfg, expert_w, dense_w):
    """Keep the tiny MoE width after InklingTextConfig.__post_init__.

    transformers InklingTextConfig (huggingface/transformers#49416): when
    dense_intermediate_size is set, intermediate_size is the routed experts'
    width and is copied onto moe_intermediate_size. Passing only
    moe_intermediate_size left the default 24576 on intermediate_size, and
    that 24576 overwrote the tiny width. Vulkan then sized each expert at
    18.0 MiB, so a two-expert budget of 62.9 KiB (COLI_VK_TIER_GB=0.00006)
    could not hold one.
    """
    cfg.moe_intermediate_size = expert_w
    cfg.intermediate_size = dense_w


def require_tiny_experts(model, expert_w, hidden):
    """Refuse a fixture whose routed experts used the real-model MoE width."""
    layers = getattr(getattr(model, "model", model), "layers", None)
    w = None
    if layers is not None:
        for layer in layers:
            ex = getattr(getattr(layer, "mlp", None), "experts", None)
            w = getattr(ex, "gate_up_proj", None) if ex is not None else None
            if w is not None:
                break
    shape = tuple(w.shape) if w is not None else None
    if shape is None or shape[-2] != 2 * expert_w or shape[-1] != hidden:
        sys.exit(
            f"tiny Inkling experts are {shape}, not [E, {2 * expert_w}, {hidden}]"
        )


def pin_saved_config(path, expert_w, dense_w):
    """HF-saved form: intermediate_size is dense, moe_intermediate_size is MoE."""
    with open(path) as f:
        saved = json.load(f)
    text = saved.get("text_config", saved)
    text["moe_intermediate_size"] = expert_w
    text["intermediate_size"] = dense_w
    text.pop("dense_intermediate_size", None)
    with open(path, "w") as f:
        json.dump(saved, f, indent=2)
        f.write("\n")


def _saved_expert_names(hdr):
    """Packed experts on a layer that exists.

    Non-wide is 8 layers with dense_mlp_idx=2, so layer 2 has experts. --wide is
    2 layers (0 dense, 1 sparse): model.layers.2.mlp.experts.gate_up_proj is
    absent. Scan like require_tiny_experts rather than hardcoding layer 2.
    """
    up = None
    for name in hdr:
        if name.endswith(".mlp.experts.gate_up_proj"):
            up = name
            break
    if up is None:
        return "model.layers.N.mlp.experts.gate_up_proj", "model.layers.N.mlp.experts.down_proj"
    return up, up[: -len("gate_up_proj")] + "down_proj"


def require_saved_tiny_experts(outdir, expert_w, hidden):
    """Refuse a snapshot whose packed experts used the real-model MoE width.

    In-memory require_tiny_experts can miss a save that still writes 24576-wide
    rows. Those rows are 18.0 MiB each; COLI_VK_TIER_GB=0.00006 (62.9 KiB) then
    holds no expert, and vulkan_engines.sh fails: no routed expert ran on the
    device.
    """
    path = os.path.join(outdir, "model.safetensors")
    with open(path, "rb") as f:
        n = struct.unpack("<Q", f.read(8))[0]
        hdr = json.loads(f.read(n))
    up, down = _saved_expert_names(hdr)
    up_shape = hdr.get(up, {}).get("shape")
    down_shape = hdr.get(down, {}).get("shape")
    if up_shape is None or len(up_shape) < 2 or up_shape[-2] != 2 * expert_w or up_shape[-1] != hidden:
        sys.exit(
            f"saved tiny Inkling {up} is {up_shape}, not [E, {2 * expert_w}, {hidden}]"
        )
    if down_shape is None or len(down_shape) < 2 or down_shape[-2] != hidden or down_shape[-1] != expert_w:
        sys.exit(
            f"saved tiny Inkling {down} is {down_shape}, not [E, {hidden}, {expert_w}]"
        )


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    wide = "--wide" in sys.argv[1:]
    out = args[0] if args else "tiny_inkling"
    torch.manual_seed(0)
    # When dense_intermediate_size is set, intermediate_size is the experts'
    # width (#49416). The tiny MoE width is 32 (64 on --wide); 24576 is the
    # real model.
    expert_w = 64 if wide else 32
    dense_w = 128 if wide else 96

    if wide:
        cfg = InklingTextConfig(
            vocab_size=256,
            unpadded_vocab_size=250,
            hidden_size=6144,
            num_hidden_layers=2,
            num_attention_heads=8,
            num_key_value_heads=2,
            head_dim=128,
            swa_num_attention_heads=8,
            swa_num_key_value_heads=4,
            swa_head_dim=128,
            sliding_window_size=8,
            d_rel=16,
            rel_extent=32,
            log_scaling_n_floor=8,
            log_scaling_alpha=0.1,
            local_layer_ids=[0],                 # global layer at 1
            dense_mlp_idx=1,
            intermediate_size=expert_w,
            dense_intermediate_size=dense_w,
            moe_intermediate_size=expert_w,
            n_routed_experts=8,
            num_experts_per_tok=2,
            n_shared_experts=2,
            route_scale=2.0,
            logits_mup_width_multiplier=4.0,
            max_position_embeddings=4096,
            eos_token_id=None,
        )
    else:
        cfg = InklingTextConfig(
            vocab_size=256,
            unpadded_vocab_size=250,
            hidden_size=64,
            num_hidden_layers=8,
            num_attention_heads=4,
            num_key_value_heads=2,
            head_dim=16,
            swa_num_attention_heads=4,
            swa_num_key_value_heads=4,
            swa_head_dim=16,
            sliding_window_size=8,
            d_rel=8,
            rel_extent=32,
            log_scaling_n_floor=8,      # small, so tau != 1 is exercised
            log_scaling_alpha=0.1,
            local_layer_ids=[0, 1, 2, 3, 4, 6, 7],   # global layer at 5
            dense_mlp_idx=2,
            intermediate_size=expert_w,
            dense_intermediate_size=dense_w,
            moe_intermediate_size=expert_w,
            n_routed_experts=8,
            num_experts_per_tok=2,
            n_shared_experts=2,
            route_scale=2.0,
            logits_mup_width_multiplier=4.0,
            max_position_embeddings=4096,
            eos_token_id=None,
        )
    pin_tiny_moe_widths(cfg, expert_w, dense_w)

    model = InklingForCausalLM(cfg).eval().float()
    require_tiny_experts(model, expert_w, cfg.hidden_size)

    prompt = [7, 42, 199, 3, 88, 154, 21, 60, 9, 133, 77, 245]
    ids = torch.tensor([prompt], dtype=torch.long)
    n_new = 24

    with torch.no_grad():
        gen = model.generate(
            ids, max_new_tokens=n_new, do_sample=False, use_cache=True
        )
        full = gen[0].tolist()
        tf = model(torch.tensor([full], dtype=torch.long)).logits[0].argmax(-1).tolist()

    model.save_pretrained(out, safe_serialization=True)
    pin_saved_config(f"{out}/config.json", expert_w, dense_w)
    require_saved_tiny_experts(out, expert_w, cfg.hidden_size)
    ref = {"prompt_ids": prompt, "full_ids": full, "tf_pred": tf}
    with open(f"{out}/ref_inkling.json", "w") as f:
        json.dump(ref, f)
    print(f"saved tiny model + ref_inkling.json to {out}/")
    print("continuation:", full[len(prompt):])


if __name__ == "__main__":
    main()
