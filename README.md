# airllm-rs

Run 70B+ parameter LLMs on a single 4-8GB GPU using Rust + CUDA.

Inspired by [AirLLM](https://github.com/lyogavin/airllm) - rewritten in Rust for performance and safety.

## Status

🚧 **Planning Phase** - Implementation pending

## Architecture

### Core Concept

Load transformer model layers **sequentially from disk** into GPU memory, run inference layer-by-layer, freeing memory between layers. This enables running massive models on minimal VRAM:

- **70B models** → 4GB VRAM
- **405B models** → 8GB VRAM

### Technical Stack

| Component | Choice |
|-----------|--------|
| **Language** | Rust |
| **Model Format** | Safetensors |
| **GPU Backend** | CUDA (via `cudarc` crate) |
| **CLI Framework** | `clap` |
| **Async Runtime** | `tokio` |
| **Tokenizer** | `tokenizers` crate |

### Key Components

1. **Model Loader**
   - Parse safetensors format
   - Layer-wise model splitting
   - Disk caching system

2. **Memory Manager**
   - GPU memory allocation/deallocation
   - Layer streaming from disk
   - Prefetching pipeline

3. **Inference Engine**
   - CUDA kernels for transformer ops
   - KV cache management
   - Token generation loop

4. **CLI Interface**
   - Model download/load commands
   - Interactive chat mode
   - Batch inference mode
   - Configuration options

## Implementation Phases

### Phase 1: Foundation
- [ ] Project setup with CUDA dependencies
- [ ] Safetensors parser
- [ ] Basic memory management

### Phase 2: Core Inference
- [ ] Layer loading/unloading
- [ ] CUDA kernel implementations
- [ ] Single-layer forward pass

### Phase 3: Full Pipeline
- [ ] Multi-layer sequential inference
- [ ] KV cache implementation
- [ ] Token generation loop

### Phase 4: Optimization
- [ ] Prefetching/async I/O
- [ ] Optional 4-bit/8-bit quantization
- [ ] Performance profiling

### Phase 5: CLI & Polish
- [ ] User-friendly CLI
- [ ] Error handling
- [ ] Documentation

## Open Decisions

- [ ] **Model architecture support:** Llama-2/Llama-3 only vs multi-architecture
- [ ] **HuggingFace integration:** Built-in download vs local files only
- [ ] **Code reuse:** Build from scratch vs leverage `candle`/`burn` crates

## Comparison: AirLLM (Python) vs airllm-rs (Rust)

| Aspect | AirLLM | airllm-rs (planned) |
|--------|--------|---------------------|
| Language | Python | Rust |
| Performance | Good | Better (native + CUDA) |
| Memory Safety | Runtime checks | Compile-time guarantees |
| Deployment | Python env required | Single binary |
| GIL Overhead | Yes | No |

## License

Planned: Apache-2.0 (matching AirLLM)

---
