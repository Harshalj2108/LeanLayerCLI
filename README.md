# LeanLayerCLI

A terminal chat interface and autonomous AI agent for local and cloud large language models, featuring an autonomous Obsidian vault memory system.

---

## What it is

**LeanLayerCLI** is a standalone, blazing-fast terminal application that transforms your command line into an intelligent agent workstation. It allows you to chat with large language models, run tools on your system, and automatically build a structured knowledge graph in Obsidian from your conversations.

Unlike typical chat interfaces, LeanLayerCLI features a two-way memory system and full agentic capabilities:

1. **Obsidian Memory Graph & Concept RAG:** When you end a session, the model summarizes the conversation, extracts key concepts, and writes structured markdown notes to an Obsidian vault. During new chats, LeanLayerCLI uses Contextual Concept RAG to dynamically scan your prompt and inject relevant vault knowledge, acting as a true long-term memory system.
2. **Interactive Session Explorer:** Easily browse past sessions, read full transcripts in a dedicated viewer, and seamlessly resume past conversations from the TUI.
3. **Autonomous Agent Tools:** The assistant isn't just text-based. It can read/write files, search your workspace, browse the web, and execute terminal commands on your behalf directly from the UI.
4. **Multi-Provider Support:** Run local models effortlessly via `llama.cpp`, or connect to cloud providers like OpenAI, Google Gemini, Anthropic, and OpenRouter.

There is no Python runtime required at runtime, no Obsidian plugins to install, and no complex configuration needed.

---

## Key Features

- 🧠 **Contextual Concept RAG & Obsidian Vault:** Automatically summarizes sessions and builds a markdown-based knowledge graph. Dynamically pulls context from past sessions when relevant concepts are mentioned.
- 🗂️ **Interactive Session Explorer:** Browse, read, and resume past sessions directly from the terminal Memory pane.
- ⚙️ **Interactive Configuration UI:** Real-time TUI for configuring models, API keys, and settings—no manual file editing required.
- 🛠️ **Agentic Tools:** The LLM can autonomously execute commands (`run_command`), edit files (`read_file`, `write_file`, `search_files`), and search the internet (`web_search`).
- ☁️ **Cloud & Local Inference:** Seamlessly switch between local GGUF models (via `llama-server`) and cloud APIs.
- 🚀 **Zero-Config Subprocess Management:** Automatically launches, manages, and terminates `llama-server` in the background.
- 🖼️ **Multimodal Support:** Supports multimodal input. Paste images directly from your clipboard to auto-save them to the vault and embed them in your prompts.
- ⚡ **Thinking Modes:** Quickly toggle between "Fast" mode for rapid responses and "Deep" mode to unlock the model's internal Chain-of-Thought reasoning.
- 📋 **Code Block Yanker:** Built-in clipboard manager. Easily extract and copy code blocks from the assistant's responses using a dedicated UI modal.
- 🖱️ **Butter-Smooth TUI:** Fast rendering with mouse scroll wheel support and intuitive shortcuts.

---

## Architecture

```text
leanlayercli chat
    │
    ▼
Rust TUI (ratatui) ─────────┐
    │                       │ HTTP API
    │ (HTTP SSE Streams)    │ (OpenAI, Gemini, Anthropic, OpenRouter)
    ▼                       ▼
llama-server (llama.cpp)   Cloud Providers
    │
    │ on session exit
    ▼
Obsidian vault (markdown + wikilinks)
```

The Rust binary handles the terminal interface, memory system, tool execution, and HTTP clients. Local inference is handled by `llama.cpp` (if configured), communicating over a local HTTP API. 

---

## Requirements

- Windows, Linux, or macOS
- Rust toolchain (for building from source)
- **Local usage:** `llama.cpp` binary (`llama-server`) and a GGUF model file. NVIDIA GPU highly recommended.
- **Cloud usage:** API keys for your preferred provider (OpenAI, Gemini, Anthropic, etc.).

---

## Installation

### 1. Clone and build

```bash
git clone https://github.com/yourname/LeanLayerCLI
cd LeanLayerCLI
cargo build --release
```

The binary will be at `target/release/leanlayercli` (or `leanlayercli.exe` on Windows).

### 2. Configure Local or Cloud Models

On first run, LeanLayerCLI creates a config file at:
- Windows: `%APPDATA%\leanlayercli\config.toml`
- Linux/macOS: `~/.config/leanlayercli/config.toml`

Run `leanlayercli config` to see the exact path.

**Option A: Cloud Providers**
Edit the configuration to use a cloud API. Set your provider (`openai`, `gemini`, `anthropic`, `openrouter`), model name, and API key. You can do this interactively via the UI:

```bash
leanlayercli config --edit
```

*(You can also use environment variables like `OPENAI_API_KEY` or `ANTHROPIC_API_KEY` instead of storing them in the config file).*

**Option B: Local Models (llama.cpp)**
If you want to run models locally, download `llama-server` from the [llama.cpp releases](https://github.com/ggml-org/llama.cpp/releases), and download a GGUF model. Then configure:

```toml
api_provider = "local"
model_path = "/path/to/your/model.gguf"
llama_server_path = "/path/to/llama-server.exe" # Auto-launch support
vault_path = "/path/to/your/obsidian/vault"
gpu_layers = 99
ctx_size = 32768
```

---

## Usage

### Start a chat session

```bash
leanlayercli chat
```

### Manage configuration

```bash
leanlayercli config          # Shows the config path
leanlayercli config --edit   # Opens the TUI in config edit mode
```

### List past sessions

```bash
leanlayercli sessions
```

---

## Interface & Controls

```text
+-- Chat ------------------------------------+-- Memory --------+
|                                           |                  |
|  You                                      |  [2026-05-03]    |
|    Find the main.rs file and read it.     |      |           |
|                                           |  [attention]     |
|  Agent                                    |    /     \       |
|    [Tool Result: search: main.rs]         |  [kv]  [softmax] |
|    I found it! Here is the content...     |                  |
|                                           |  [2026-05-01]    |
+-------------------------------------------+------------------+
|  > _                                                         |
+--------------------------------------------------------------+
|  leanlayercli Ready [FAST]  m: toggle mode   tab: switch   q: quit |
```

- **Left panel:** Chat history with streaming tokens and inline tool execution results.
- **Right panel:** Memory graph showing linked sessions and concepts. You can select past sessions to read or resume them.
- **Bottom bar:** Input box and keyboard shortcuts.

### Keyboard Shortcuts

| Key               | Action                                          |
|-------------------|-------------------------------------------------|
| `Enter`           | Send message                                    |
| `Shift+Enter`     | Insert newline                                  |
| `m`               | Toggle thinking mode (Fast/Deep)                |
| `q` / `Ctrl+C`    | Quit and automatically save session to vault    |
| `Tab`             | Switch focus between Chat and Memory panels     |
| `Up/Down`         | Scroll chat history or navigate Memory graph    |
| `Mouse Scroll`    | Scroll history / graph seamlessly               |
| `Ctrl+Y`          | Open Code Block Yanker modal                    |
| `Ctrl+Shift+C`    | Yank selected code block in Yanker modal        |
| `Ctrl+V`          | Paste text or images into chat                  |
| `r`               | Resume an old session (in Session Viewer modal) |

---

## Autonomous Agent Tools

LeanLayerCLI is fully equipped with an agentic tool system. The model can natively output structured JSON blocks to interact with your system.

**Available Tools:**
1. `run_command`: Executes shell commands (`cargo build`, `pytest`, `git status`).
2. `read_file`: Reads arbitrary files on your disk.
3. `write_file`: Creates or modifies code files.
4. `search_files`: Greps through workspace files for patterns.
5. `web_search`: Searches the web for up-to-date context.

*Note: You are always prompted to confirm risky actions (like running commands or writing files) before they are executed.*

---

## Memory System (Obsidian Vault)

When you quit (`q`), the model receives the full conversation transcript and is asked to produce a JSON summary containing:
- A brief summary of the discussion.
- A list of key concepts mentioned.
- Related topics.

This runs as a background thread, immediately closing the terminal and writing Markdown files natively into your Obsidian Vault (`sessions/` and `concepts/`).

### Contextual Concept RAG
At the start of each new session, LeanLayerCLI dynamically loads the most recent concept nodes. Even more powerfully, when you type a prompt, it scans for known concepts and instantly injects the relevant historical transcripts into the system prompt. Over time, your AI builds a robust, long-term memory graph spanning all of your interactions.

---

## License

Apache-2.0