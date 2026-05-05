# airllm-rs

A terminal chat interface for local large language models with autonomous Obsidian vault memory.

---

## What it is

airllm-rs is a standalone terminal application that lets you chat with large language models running entirely on your machine. When you end a session, the model summarizes the conversation, extracts key concepts, and writes structured markdown notes to an Obsidian vault — automatically building a knowledge graph of everything you have discussed across sessions.

You do not configure anything between sessions. You do not paste summaries manually. You do not open Obsidian to feed context back to the model. The tool handles all of that on exit.

---

## How it differs from existing tools

Most tools that combine AI with Obsidian work in one direction: they read your existing vault and use it to give the AI context. Sonar, for example, is an Obsidian plugin that searches your notes to answer questions. OpenClaw integrations require you to manually maintain memory files and inject them into prompts.

airllm-rs inverts this. The AI writes to Obsidian. You chat, and the vault grows on its own. Each session becomes a node. Concepts become linked pages. Sessions that share topics connect to each other through wikilinks. Over time, the vault becomes a structured record of everything you have thought through with the model.

There is no plugin to install. No Obsidian open in the background. Just a single binary.

---

## Architecture

```
airllm chat
    |
    v
Rust TUI (ratatui)
    |
    | HTTP streaming (OpenAI-compatible API)
    v
llama-server (llama.cpp)
    |
    | CUDA + CPU offload
    v
Local model (GGUF format)
    |
    | on session exit
    v
Obsidian vault (markdown + wikilinks)
```

The Rust binary handles the terminal interface and memory system. llama.cpp handles inference. The two communicate over a local HTTP API on port 8081. No Python runtime is required at runtime.

---

## Requirements

- Windows, Linux, or macOS
- Rust toolchain (for building from source)
- llama.cpp binary (llama-server)
- A GGUF model file
- NVIDIA GPU recommended (CPU-only works but is slow)
- CUDA toolkit if using GPU acceleration

---

## Installation

### 1. Clone and build

```bash
git clone https://github.com/yourname/airllm-rs
cd airllm-rs
cargo build --release
```

The binary will be at `target/release/airllm` (or `airllm.exe` on Windows).

### 2. Download llama.cpp

Download the latest release for your platform from:
```
https://github.com/ggml-org/llama.cpp/releases/latest
```

For Windows with NVIDIA GPU, download both:
- `llama-bXXXX-bin-win-cuda-12.4-x64.zip`
- `cudart-llama-bin-win-cuda-12.4-x64.zip`

Extract both into the same directory.

### 3. Download a model

Any GGUF model works. The project was developed with:
```
TeichAI/Qwen3.6-27B-Claude-Opus-Reasoning-Distill-v2-GGUF
```

Download using the Hugging Face CLI:
```bash
hf download TeichAI/Qwen3.6-27B-Claude-Opus-Reasoning-Distill-v2-GGUF --local-dir /path/to/models
```

For machines with less than 32GB RAM, use the Q4_K_M quantization. It reduces the model to roughly 15GB with minimal quality loss.

### 4. Configure

On first run, airllm-rs creates a config file at:
- Windows: `%APPDATA%\airllm\config.toml`
- Linux/macOS: `~/.config/airllm/config.toml`

Edit it to set your paths:

```toml
model_path = "/path/to/your/model.gguf"
vault_path = "/path/to/your/obsidian/vault"
max_context_nodes = 5
summarize_on_exit = true
backend_script = "airllm_backend.py"
```

---

## Usage

### Start llama-server

Before running airllm-rs, start llama-server with your model:

```bash
# Linux/macOS
./llama-server -m /path/to/model.gguf --port 8081 -ngl 99 --ctx-size 32768 --no-warmup

# Windows
llama-server.exe -m C:\path\to\model.gguf --port 8081 -ngl 99 --ctx-size 32768 --no-warmup
```

Adjust `-ngl` (number of GPU layers) based on your VRAM:

| VRAM   | Recommended -ngl |
|--------|-----------------|
| 4 GB   | 10              |
| 6 GB   | 20              |
| 8 GB   | 28              |
| 12 GB  | 40              |
| 16 GB+ | 99 (all layers) |

### Start a chat session

```bash
airllm chat
```

### List past sessions

```bash
airllm sessions
```

### Show config file location

```bash
airllm config
```

---

## Interface

```
+-- Chat ------------------------------------+-- Memory --------+
|                                           |                  |
|  You                                      |  [2026-05-03]    |
|    how does attention work                |      |           |
|                                           |  [attention]     |
|  Qwen                                     |    /     \       |
|    Attention allows each token to         |  [kv]  [softmax] |
|    selectively weight other tokens...     |                  |
|                                           |  [2026-05-01]    |
+-------------------------------------------+------------------+
|  > _                                                         |
+--------------------------------------------------------------+
|  airllm   Ready   [FAST]   m: toggle mode   tab: switch   q: quit |
```

- Left panel: chat history with automatic word-wrapping and streaming token display
- Right panel: memory graph showing linked sessions and concepts
- Bottom bar: input and keyboard shortcuts

### Keyboard shortcuts

| Key       | Action                        |
|-----------|-------------------------------|
| Enter     | Send message                  |
| m         | Toggle thinking mode (Fast/Deep) |
| q         | Quit and save session to vault |
| Ctrl+C    | Quit and save session to vault |
| Tab       | Switch focus between panels   |
| Up / Down | Scroll chat history           |
| Mouse Scroll| Scroll chat history (faster) |

### Thinking Modes

airllm-rs natively supports toggling between two different reasoning configurations for local models:
- **Fast Mode (`[FAST]`)**: The default. Optimized for quick queries. Uses a lower temperature (0.6), disables backend thinking parameters, and enforces a streaming filter that actively strips any leaked `<think>` tokens from the output so you only see the final response.
- **Deep Mode (`[DEEP]`)**: Optimized for coding, math, and complex logic. Increases the temperature (0.7), enables backend thinking structures, and perfectly streams the model's internal chain-of-thought to your screen before it provides the final answer.

---

## Memory system

### How it works

When you quit, the model receives the full conversation transcript and is asked to produce a JSON summary containing:
- A 2-3 sentence summary of what was discussed
- A list of key concepts mentioned
- A list of related topics the conversation connects to

This runs as a background thread so the terminal closes immediately. The vault is updated after you have already exited.

### Vault structure

```
your-vault/
    sessions/
        2026-05-03-143022.md
        2026-05-04-091145.md
    concepts/
        attention-mechanism.md
        kv-cache.md
        transformer-architecture.md
    .obsidian/
```

### Session note format

```markdown
---
date: 2026-05-03 14:30
model: qwen36-claude
tags:
  - attention
  - transformers
  - kv-cache
related: [[attention-mechanism]], [[kv-cache]]
---

# Session: 2026-05-03-143022

## Summary
Discussed how the attention mechanism works in transformer models,
focusing on the role of key-value caching during autoregressive generation.

## Key Concepts
- [[attention-mechanism]]
- [[kv-cache]]
- [[transformer-architecture]]

## Transcript
**You**: how does attention work

**Qwen**: Attention allows each token to selectively weight...
```

### Concept note format

```markdown
---
concept: kv-cache
first_seen: 2026-05-03
---

# kv-cache

## Sessions
- [[sessions/2026-05-03-143022]]
- [[sessions/2026-05-04-091145]]
```

Wikilinks between sessions and concepts are fully compatible with Obsidian's graph view. Open the vault folder in Obsidian at any time to browse the graph visually.

### Context injection

At the start of each new session, airllm-rs loads the N most recent session nodes and their connected concepts (configurable via `max_context_nodes` in config). This is injected as a system message so the model has awareness of past conversations without you doing anything.

---

## Performance

Generation speed depends primarily on how many layers fit in VRAM. With a 27B Q4_K_M model:

| Setup                        | Approximate speed     |
|------------------------------|-----------------------|
| All layers on GPU (16GB+)    | 15-25 tokens/sec      |
| Mixed GPU/CPU (8GB VRAM)     | 5-10 tokens/sec       |
| Mixed GPU/CPU (6GB VRAM)     | 2-4 tokens/sec        |
| CPU only                     | 0.5-1.5 tokens/sec    |

Closing GPU-heavy background applications (browsers, game launchers, streaming apps) frees VRAM and meaningfully improves speed.

---

## Model compatibility

airllm-rs works with any model that llama.cpp supports in GGUF format. This includes:

- Qwen 2.5 / Qwen 3 series
- Llama 3 / Llama 3.1 / Llama 3.3
- Mistral and Mixtral
- Phi-3 / Phi-4
- Gemma 2 / Gemma 3
- DeepSeek R1 series
- Any fine-tune of the above in GGUF format

The model used during development is `TeichAI/Qwen3.6-27B-Claude-Opus-Reasoning-Distill-v2`, a fine-tune of Qwen3.6-27B trained on Claude Opus reasoning data.

---

## Project structure

```
airllm-rs/
    src/
        main.rs               -- CLI entry point
        config.rs             -- Config loading and defaults
        lib.rs                -- Crate root
        backend/
            mod.rs
            process.rs        -- HTTP client for llama-server
            protocol.rs       -- Message types
        memory/
            mod.rs
            vault.rs          -- Obsidian markdown writer
            graph.rs          -- Node/edge graph loader
            summarize.rs      -- End-of-session summarization
        tui/
            mod.rs            -- TUI event loop
            app.rs            -- Application state
            layout.rs         -- Panel rendering
            graph.rs          -- ASCII graph renderer
    Cargo.toml
    README.md
```

---

## Design decisions

**Why llama.cpp instead of a Python inference stack**

llama.cpp handles GPU/CPU memory splitting automatically and supports every major model architecture. Building custom CUDA kernels in Rust for a single architecture would have taken months and supported fewer models. llama.cpp is battle-tested and actively maintained.

**Why Rust for the TUI**

ratatui and crossterm provide a complete, well-maintained TUI stack in Rust. The binary is small, starts instantly, and has no runtime dependencies beyond the llama-server process.

**Why Obsidian vault format**

Obsidian's format is plain markdown with wikilinks. It requires no database, no server, and no proprietary format. The vault is fully human-readable without Obsidian installed. If you do have Obsidian, you get the graph view for free.

**Why the model summarizes itself**

Having the model extract its own concepts and generate its own wikilinks produces more semantically accurate memory nodes than keyword extraction or embedding-based approaches. The model understands what was actually important in the conversation.

---

## License

Apache-2.0