#!/usr/bin/env python3
"""
airllm_backend.py — Layer-streaming inference backend for Gemma 4 31B.
Replaces AirLLM's broken optimum/BetterTransformer dependency with
PyTorch native SDPA (torch >= 2.0) + safetensors direct loading.

Protocol: JSON-lines on stdin/stdout.
"""

import sys
import json
import os
import gc
import torch
from pathlib import Path
from transformers import AutoTokenizer, AutoConfig, AutoModelForCausalLM
from safetensors.torch import load_file


# ── Config ────────────────────────────────────────────────────────────────────

DEVICE = "cuda" if torch.cuda.is_available() else "cpu"
DTYPE  = torch.float16   # bfloat16 also fine on RTX 30/40 series

# ── Helpers ───────────────────────────────────────────────────────────────────

def send(obj: dict):
    print(json.dumps(obj), flush=True)

def log(msg: str):
    sys.stderr.write(f"[backend] {msg}\n")
    sys.stderr.flush()


# ── Model loader ──────────────────────────────────────────────────────────────

class LayerStreamingModel:
    """
    Loads a HuggingFace model layer-by-layer from disk, keeping only
    one layer in GPU VRAM at a time. No BetterTransformer, no optimum.
    Uses PyTorch native SDPA (enabled by default in transformers >= 4.36).
    """

    def __init__(self, model_path: str):
        self.model_path = Path(model_path)
        log(f"Loading tokenizer from {model_path}")
        self.tokenizer = AutoTokenizer.from_pretrained(model_path)
        self.config = AutoConfig.from_pretrained(model_path)

        # Load the full model onto CPU with meta device trick —
        # weights stay on disk, only config/structure in RAM.
        log("Mapping model structure (meta device)...")
        with torch.device("meta"):
            self.model = AutoModelForCausalLM.from_config(self.config)

        self.model.eval()

        # Find safetensors shards
        self.shards = sorted(self.model_path.glob("*.safetensors"))
        if not self.shards:
            raise FileNotFoundError(f"No .safetensors files found in {model_path}")
        log(f"Found {len(self.shards)} safetensors shard(s)")

        # Build param→shard index
        log("Building weight index...")
        self.weight_index = self._build_weight_index()
        log("Model ready for streaming inference")

    def _build_weight_index(self) -> dict:
        """Map each weight name to which shard file contains it."""
        index = {}
        for shard in self.shards:
            # peek at keys without loading tensors
            from safetensors import safe_open
            with safe_open(shard, framework="pt", device="cpu") as f:
                for key in f.keys():
                    index[key] = shard
        return index

    def _load_weights_for_layer(self, layer_idx: int) -> dict:
        """Load only the weights needed for a specific transformer layer."""
        prefix = f"model.layers.{layer_idx}."
        needed = {k: v for k, v in self.weight_index.items() if k.startswith(prefix)}

        tensors = {}
        shards_needed = set(needed.values())
        for shard in shards_needed:
            from safetensors import safe_open
            with safe_open(shard, framework="pt", device=DEVICE) as f:
                for key in f.keys():
                    if key in needed:
                        tensors[key] = f.get_tensor(key).to(DTYPE)
        return tensors

    def _load_non_layer_weights(self) -> dict:
        """Load embed, lm_head, norm — small enough to keep in VRAM."""
        patterns = ["model.embed_tokens", "model.norm", "lm_head"]
        tensors = {}
        for shard in self.shards:
            from safetensors import safe_open
            with safe_open(shard, framework="pt", device=DEVICE) as f:
                for key in f.keys():
                    if any(key.startswith(p) for p in patterns):
                        tensors[key] = f.get_tensor(key).to(DTYPE)
        return tensors

    @torch.inference_mode()
    def generate(self, messages: list, max_new_tokens: int = 1024):
        """
        Layer-streaming generation. Yields token strings.
        Uses transformers chat template for proper Gemma 4 formatting.
        """
        # Format with chat template
        input_ids = self.tokenizer.apply_chat_template(
            messages,
            add_generation_prompt=True,
            return_tensors="pt",
        ).to(DEVICE)

        # Load permanent weights (embed + norm + lm_head)
        static = self._load_non_layer_weights()
        embed_weight = static["model.embed_tokens.weight"]

        # Embed input tokens
        hidden = torch.nn.functional.embedding(input_ids, embed_weight)

        num_layers = self.config.num_hidden_layers
        generated_ids = []

        for _ in range(max_new_tokens):
            x = hidden  # [batch, seq, hidden]

            # Stream each layer
            for layer_idx in range(num_layers):
                layer_weights = self._load_weights_for_layer(layer_idx)

                # Temporarily assign weights to the layer module
                layer = self.model.model.layers[layer_idx]
                for name, tensor in layer_weights.items():
                    # Strip prefix to get local param name
                    local_name = name[len(f"model.layers.{layer_idx}."):]
                    parts = local_name.split(".")
                    obj = layer
                    for part in parts[:-1]:
                        obj = getattr(obj, part)
                    # Replace the param with a real tensor
                    setattr(obj, parts[-1], torch.nn.Parameter(tensor, requires_grad=False))

                # Forward through this layer
                # position_ids for correct RoPE
                seq_len = x.shape[1]
                position_ids = torch.arange(seq_len, device=DEVICE).unsqueeze(0)

                layer_out = layer(
                    x,
                    position_ids=position_ids,
                    use_cache=False,
                )
                x = layer_out[0]

                # Free GPU memory for this layer immediately
                del layer_weights
                gc.collect()
                torch.cuda.empty_cache()

            # Final norm + lm_head
            norm_weight = static["model.norm.weight"]
            lm_head_weight = static["lm_head.weight"]

            # RMS norm
            variance = x.pow(2).mean(-1, keepdim=True)
            x_normed = x * torch.rsqrt(variance + self.config.rms_norm_eps)
            x_normed = x_normed * norm_weight

            # Get logits for last token only
            logits = torch.nn.functional.linear(x_normed[:, -1:, :], lm_head_weight)
            next_token = logits[0, -1].argmax(dim=-1).unsqueeze(0).unsqueeze(0)

            token_id = next_token.item()
            generated_ids.append(token_id)

            # Decode and yield
            token_str = self.tokenizer.decode([token_id], skip_special_tokens=False)
            yield token_str

            # EOS check
            if token_id == self.tokenizer.eos_token_id:
                break

            # Append new token embedding for next step
            new_embed = torch.nn.functional.embedding(next_token, embed_weight)
            hidden = torch.cat([hidden, new_embed], dim=1)

        del static
        gc.collect()
        torch.cuda.empty_cache()


# ── Main loop ─────────────────────────────────────────────────────────────────

def main():
    if len(sys.argv) < 2:
        sys.stderr.write("Usage: airllm_backend.py <model_path>\n")
        sys.exit(1)

    model_path = sys.argv[1]

    try:
        model = LayerStreamingModel(model_path)
    except Exception as e:
        send({"type": "error", "message": f"Failed to load model: {e}"})
        sys.exit(1)

    send({"type": "ready"})

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            send({"type": "error", "message": f"invalid JSON: {line}"})
            continue

        if msg.get("type") == "generate":
            messages = msg.get("messages", [])
            # Filter system messages with role mapping
            formatted = [
                {"role": m["role"], "content": m["content"]}
                for m in messages
                if m["role"] in ("user", "assistant", "system")
            ]
            try:
                for token in model.generate(formatted):
                    send({"type": "token", "content": token})
                send({"type": "done"})
            except Exception as e:
                send({"type": "error", "message": str(e)})

        elif msg.get("type") == "ping":
            send({"type": "pong"})

        elif msg.get("type") == "quit":
            break


if __name__ == "__main__":
    main()