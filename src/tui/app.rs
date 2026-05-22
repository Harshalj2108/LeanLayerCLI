use anyhow::Result;
use ratatui::crossterm::event::{KeyCode, KeyEvent, MouseEvent, MouseEventKind, KeyModifiers};
use tokio::sync::mpsc;
use tui_textarea::TextArea;
use arboard::Clipboard;

use crate::backend::{
    process::Backend,
    protocol::{BackendMessage, ChatMessage},
};
use crate::config::Config;
use crate::memory::{
    graph::MemoryGraph,
    summarize::summarize_session,
    vault::VaultWriter,
};

pub enum Focus {
    Chat,
    Graph,
}

#[derive(Clone)]
pub enum ModalState {
    SessionViewer {
        title: String,
        content: String,
        scroll: usize,
        node_id: String,
        is_session: bool,
    },
    ToolGatekeeper {
        call: crate::agent::tools::ToolCall,
        pending_others: Vec<crate::agent::tools::ToolCall>,
    },
    CodeGatekeeper {
        request: crate::agent::executor::ExecutionRequest,
        pending_others: Vec<crate::agent::executor::ExecutionRequest>,
    },
    ConfigEditor {
        active_field: usize,
        is_editing: bool,
        cfg_draft: crate::config::Config,
    },
    CodeBlockYanker {
        blocks: Vec<(String, String)>,
        selected_index: usize,
        preview_scroll: usize,
    },
}

pub struct App<'a> {
    pub cfg: Config,
    pub messages: Vec<ChatMessage>,
    pub input: TextArea<'a>,
    pub focus: Focus,
    pub is_generating: bool,
    pub current_response: String,
    pub status: String,
    pub should_quit: bool,
    pub graph: MemoryGraph,
    pub thinking_mode: bool,
    backend: Backend,
    pub scroll: usize,
    pub graph_scroll: usize,
    pub selected_node_index: usize,
    pub active_modal: Option<ModalState>,
    pub token_rx: Option<mpsc::UnboundedReceiver<BackendMessage>>,
    clipboard: Option<Clipboard>,
}

impl<'a> App<'a> {
    pub async fn new(cfg: Config) -> Result<Self> {
        let backend = Backend::spawn(&cfg)?;
        let graph = MemoryGraph::load(&std::path::PathBuf::from(&cfg.vault_path))?;

        let mut messages = Vec::new();

        // Inject tool-calling system prompt
        let tool_prompt = crate::agent::tools::build_tool_system_prompt();
        messages.push(ChatMessage {
            role: "system".into(),
            content: tool_prompt,
            images: None,
        });

        let context = build_context(&graph, cfg.max_context_nodes);
        if !context.is_empty() {
            messages.push(ChatMessage {
                role: "system".into(),
                content: context,
                images: None,
            });
        }

        let mut input = TextArea::default();
        input.set_cursor_line_style(ratatui::style::Style::default());
        input.set_placeholder_text(" Type your message here... (Enter to submit, Shift+Enter for newline, Ctrl+V to paste)");

        Ok(Self {
            cfg,
            messages,
            input,
            focus: Focus::Chat,
            is_generating: false,
            current_response: String::new(),
            status: "Ready".into(),
            should_quit: false,
            graph,
            thinking_mode: false,
            backend,
            scroll: 0,
            graph_scroll: 0,
            selected_node_index: 0,
            active_modal: None,
            token_rx: None,
            clipboard: Clipboard::new().ok(),
        })
    }

    pub fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Chat => Focus::Graph,
            Focus::Graph => Focus::Chat,
        };
    }

    pub fn toggle_thinking_mode(&mut self) {
        self.thinking_mode = !self.thinking_mode;
    }

    pub fn tick(&mut self) {
        if let Some(rx) = &mut self.token_rx {
            loop {
                match rx.try_recv() {
                    Ok(BackendMessage::Token { content }) => {
                        self.current_response.push_str(&content);
                    }
                    Ok(BackendMessage::Done) => {
                        self.messages.push(ChatMessage {
                            role: "assistant".into(),
                            content: self.current_response.clone(),
                            images: None,
                        });
                        
                        let tool_calls = crate::agent::tools::parse_tool_calls(&self.current_response);
                        if !tool_calls.is_empty() {
                            let mut pending = tool_calls;
                            let first = pending.remove(0);
                            self.active_modal = Some(ModalState::ToolGatekeeper {
                                call: first,
                                pending_others: pending,
                            });
                        } else {
                            let exec_blocks = crate::agent::executor::detect_executable_blocks(&self.current_response);
                            if !exec_blocks.is_empty() {
                                let mut pending = exec_blocks;
                                let first = pending.remove(0);
                                self.active_modal = Some(ModalState::CodeGatekeeper {
                                    request: first,
                                    pending_others: pending,
                                });
                            }
                        }

                        self.current_response.clear();
                        self.is_generating = false;
                        self.status = "Ready".into();
                        self.token_rx = None;
                        break;
                    }
                    Ok(BackendMessage::Error { message }) => {
                        self.status = format!("Error: {}", message);
                        self.is_generating = false;
                        self.token_rx = None;
                        break;
                    }
                    Ok(_) => {}
                    Err(mpsc::error::TryRecvError::Empty) => break,
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        self.is_generating = false;
                        self.token_rx = None;
                        break;
                    }
                }
            }
        }
    }

    pub fn handle_paste(&mut self, text: String) {
        if matches!(self.focus, Focus::Chat) {
            for (i, line) in text.lines().enumerate() {
                if i > 0 {
                    self.input.insert_newline();
                }
                self.input.insert_str(line);
            }
        }
    }

    pub async fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        if let Some(modal) = self.active_modal.clone() {
            match modal {
                ModalState::SessionViewer { title, content, mut scroll, node_id, is_session } => {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('q') => {
                            self.active_modal = None;
                            self.status = "Ready".into();
                        }
                        KeyCode::Up => {
                            if let Some(ModalState::SessionViewer { scroll: s, .. }) = &mut self.active_modal {
                                *s = s.saturating_sub(1);
                            }
                        }
                        KeyCode::Down => {
                            if let Some(ModalState::SessionViewer { scroll: s, .. }) = &mut self.active_modal {
                                *s += 1;
                            }
                        }
                        KeyCode::Char('r') if is_session => {
                            self.active_modal = None;
                            self.resume_session(&node_id, &content)?;
                        }
                        _ => {}
                    }
                }
                ModalState::ToolGatekeeper { call, pending_others } => {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('n') => {
                            self.messages.push(ChatMessage {
                                role: "user".into(),
                                content: crate::agent::tools::format_tool_result(&call, "Execution denied by user.", false),
                                images: None,
                            });
                            if pending_others.is_empty() {
                                self.active_modal = None;
                                self.submit()?;
                            } else {
                                let mut p = pending_others;
                                let next = p.remove(0);
                                self.active_modal = Some(ModalState::ToolGatekeeper { call: next, pending_others: p });
                            }
                        }
                        KeyCode::Enter | KeyCode::Char('y') => {
                            self.status = "Executing tool...".into();
                            let result = match &call {
                                crate::agent::tools::ToolCall::RunCommand { command, working_dir } => {
                                    let dir = working_dir.clone().unwrap_or_else(|| ".".into());
                                    crate::agent::executor::execute_tool_command(command, &dir)
                                }
                                crate::agent::tools::ToolCall::ReadFile { path } => {
                                    crate::agent::executor::read_file_global(path)
                                }
                                crate::agent::tools::ToolCall::WriteFile { path, content } => {
                                    crate::agent::executor::write_file_global(path, content)
                                }
                                crate::agent::tools::ToolCall::SearchFiles { query: _, .. } => {
                                    Ok(crate::agent::executor::ExecutionStatus::Failed("Search not natively implemented".into()))
                                }
                                crate::agent::tools::ToolCall::WebSearch { query } => {
                                    self.status = format!("Searching the web: {}...", query);
                                    crate::agent::executor::execute_web_search(query)
                                }
                            };
                            
                            let (out, success) = match result {
                                Ok(crate::agent::executor::ExecutionStatus::Completed { stdout, stderr, exit_code }) => {
                                    let mut s = stdout;
                                    if !stderr.is_empty() { s.push_str("\n--- STDERR ---\n"); s.push_str(&stderr); }
                                    (s, exit_code == 0)
                                }
                                Ok(crate::agent::executor::ExecutionStatus::Failed(e)) => (e, false),
                                Err(e) => (e.to_string(), false),
                                _ => ("Unknown error".into(), false),
                            };

                            self.messages.push(ChatMessage {
                                role: "user".into(),
                                content: crate::agent::tools::format_tool_result(&call, &out, success),
                                images: None,
                            });
                            
                            if pending_others.is_empty() {
                                self.active_modal = None;
                                self.submit()?;
                            } else {
                                let mut p = pending_others;
                                let next = p.remove(0);
                                self.active_modal = Some(ModalState::ToolGatekeeper { call: next, pending_others: p });
                            }
                        }
                        _ => {}
                    }
                }
                ModalState::CodeGatekeeper { request, pending_others } => {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('n') => {
                            self.active_modal = None;
                            self.status = "Ready".into();
                        }
                        KeyCode::Enter | KeyCode::Char('y') => {
                            match crate::agent::executor::execute_code(&request) {
                                Ok(crate::agent::executor::ExecutionStatus::Completed { stdout, stderr, .. }) => {
                                    self.messages.push(ChatMessage {
                                        role: "user".into(),
                                        content: format!("Execution result:\nSTDOUT:\n{}\nSTDERR:\n{}", stdout, stderr),
                                        images: None,
                                    });
                                    self.active_modal = None;
                                    self.submit()?;
                                }
                                _ => {
                                    self.active_modal = None;
                                }
                            }
                        }
                        _ => {}
                    }
                }
                ModalState::ConfigEditor { mut active_field, mut is_editing, mut cfg_draft } => {
                    if is_editing {
                        match key.code {
                            KeyCode::Esc => {
                                is_editing = false;
                            }
                            KeyCode::Enter => {
                                let val = self.input.lines().join("");
                                match active_field {
                                    0 => cfg_draft.model_path = val,
                                    1 => cfg_draft.vault_path = val,
                                    2 => cfg_draft.llama_server_path = if val.is_empty() { None } else { Some(val) },
                                    7 => cfg_draft.api_provider = val,
                                    8 => cfg_draft.api_key = if val.is_empty() { None } else { Some(val) },
                                    9 => cfg_draft.api_model = if val.is_empty() { None } else { Some(val) },
                                    _ => {}
                                }
                                is_editing = false;
                                self.input = tui_textarea::TextArea::default();
                            }
                            _ => {
                                self.input.input(key);
                            }
                        }
                    } else {
                        match key.code {
                            KeyCode::Esc | KeyCode::Char('q') => {
                                self.active_modal = None;
                                return Ok(());
                            }
                            KeyCode::Up => {
                                active_field = active_field.saturating_sub(1);
                            }
                            KeyCode::Down => {
                                active_field = (active_field + 1).min(9);
                            }
                            KeyCode::Left => {
                                match active_field {
                                    3 => cfg_draft.gpu_layers = cfg_draft.gpu_layers.saturating_sub(1),
                                    4 => cfg_draft.ctx_size = cfg_draft.ctx_size.saturating_sub(512),
                                    5 => cfg_draft.port = cfg_draft.port.saturating_sub(1),
                                    6 => cfg_draft.summarize_on_exit = !cfg_draft.summarize_on_exit,
                                    _ => {}
                                }
                            }
                            KeyCode::Right => {
                                match active_field {
                                    3 => cfg_draft.gpu_layers += 1,
                                    4 => cfg_draft.ctx_size += 512,
                                    5 => cfg_draft.port += 1,
                                    6 => cfg_draft.summarize_on_exit = !cfg_draft.summarize_on_exit,
                                    _ => {}
                                }
                            }
                            KeyCode::Enter => {
                                if active_field <= 2 || (active_field >= 7 && active_field <= 9) {
                                    is_editing = true;
                                    self.input = tui_textarea::TextArea::default();
                                    let current_val = match active_field {
                                        0 => &cfg_draft.model_path,
                                        1 => &cfg_draft.vault_path,
                                        2 => cfg_draft.llama_server_path.as_deref().unwrap_or(""),
                                        7 => &cfg_draft.api_provider,
                                        8 => cfg_draft.api_key.as_deref().unwrap_or(""),
                                        9 => cfg_draft.api_model.as_deref().unwrap_or(""),
                                        _ => "",
                                    };
                                    self.input.insert_str(current_val);
                                }
                            }
                            KeyCode::Char('s') if key.modifiers.contains(ratatui::crossterm::event::KeyModifiers::CONTROL) => {
                                // Save!
                                if let Ok(_) = crate::config::save(&cfg_draft) {
                                    self.cfg = cfg_draft.clone();
                                    self.status = "Configuration saved successfully.".into();
                                    self.active_modal = None;
                                    return Ok(());
                                } else {
                                    self.status = "Failed to save configuration.".into();
                                }
                            }
                            _ => {}
                        }
                    }
                    self.active_modal = Some(ModalState::ConfigEditor { active_field, is_editing, cfg_draft });
                }
                ModalState::CodeBlockYanker { blocks, selected_index: _, preview_scroll: _ } => {
                    match key.code {
                        KeyCode::Esc => {
                            self.active_modal = None;
                            self.status = "Ready".into();
                        }
                        KeyCode::Up => {
                            if let Some(ModalState::CodeBlockYanker { selected_index: idx, preview_scroll: ps, .. }) = &mut self.active_modal {
                                if !blocks.is_empty() {
                                    *idx = idx.saturating_sub(1);
                                    *ps = 0;
                                }
                            }
                        }
                        KeyCode::Down => {
                            if let Some(ModalState::CodeBlockYanker { selected_index: idx, preview_scroll: ps, blocks }) = &mut self.active_modal {
                                if !blocks.is_empty() {
                                    *idx = (*idx + 1).min(blocks.len() - 1);
                                    *ps = 0;
                                }
                            }
                        }
                        KeyCode::PageUp => {
                            if let Some(ModalState::CodeBlockYanker { preview_scroll: ps, .. }) = &mut self.active_modal {
                                *ps = ps.saturating_sub(5);
                            }
                        }
                        KeyCode::PageDown => {
                            if let Some(ModalState::CodeBlockYanker { preview_scroll: ps, .. }) = &mut self.active_modal {
                                *ps = ps.saturating_add(5);
                            }
                        }
                        _ => {
                            let is_ctrl_shift_c = (key.code == KeyCode::Char('c') || key.code == KeyCode::Char('C')) 
                                && (key.modifiers.contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT) || (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('C')));
                            
                            if is_ctrl_shift_c {
                                if let Some(ModalState::CodeBlockYanker { selected_index: idx, blocks, .. }) = &mut self.active_modal {
                                    if let Some((lang, code)) = blocks.get(*idx) {
                                        if let Some(clipboard) = &mut self.clipboard {
                                            if clipboard.set_text(code.clone()).is_ok() {
                                                self.status = format!("Copied {} code block to clipboard!", lang);
                                            } else {
                                                self.status = "Failed to copy to clipboard".into();
                                            }
                                        } else {
                                            self.status = "Clipboard not available".into();
                                        }
                                    }
                                }
                                self.active_modal = None;
                            }
                        }
                    }
                }
            }
            return Ok(());
        }

        match self.focus {
            Focus::Chat => {
                if key.code == KeyCode::Enter && key.modifiers.is_empty() {
                    self.submit()?;
                } else if key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.input.insert_newline();
                } else if (key.code == KeyCode::Char('v') || key.code == KeyCode::Char('V'))
                    && (key.modifiers.contains(KeyModifiers::CONTROL) || key.modifiers.contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT)) {
                    if let Some(clipboard) = &mut self.clipboard {
                        if let Ok(text) = clipboard.get_text() {
                            self.handle_paste(text);
                        } else if let Ok(image) = clipboard.get_image() {
                            // Save image to vault and insert markdown reference
                            match self.save_clipboard_image(&image) {
                                Ok(path) => {
                                    let md_ref = format!("![image]({})", path);
                                    self.input.insert_str(&md_ref);
                                    self.status = format!("Image pasted: {}x{}", image.width, image.height);
                                }
                                Err(e) => {
                                    self.status = format!("Failed to save image: {}", e);
                                }
                            }
                        }
                    }
                } else if (key.code == KeyCode::Char('y') || key.code == KeyCode::Char('Y')) && key.modifiers.contains(KeyModifiers::CONTROL) {
                    let blocks = self.extract_code_blocks();
                    if blocks.is_empty() {
                        self.status = "No code blocks found in the last assistant response".into();
                    } else {
                        self.active_modal = Some(ModalState::CodeBlockYanker {
                            blocks,
                            selected_index: 0,
                            preview_scroll: 0,
                        });
                        self.status = "Select a code block to copy".into();
                    }
                } else if key.code == KeyCode::Up && key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.scroll += 1;
                } else if key.code == KeyCode::Down && key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.scroll = self.scroll.saturating_sub(1);
                } else {
                    self.input.input(key);
                }
            },
            Focus::Graph => match key.code {
                KeyCode::Up => {
                    self.selected_node_index = self.selected_node_index.saturating_sub(1);
                }
                KeyCode::Down => {
                    let total_nodes = self.graph.recent_nodes(8).len();
                    if total_nodes > 0 {
                        self.selected_node_index = (self.selected_node_index + 1).min(total_nodes - 1);
                    }
                }
                KeyCode::Enter => {
                    let nodes = self.graph.recent_nodes(8);
                    if let Some(node) = nodes.get(self.selected_node_index) {
                        let node_id = node.id.clone();
                        let is_session = match node.kind {
                            crate::memory::graph::NodeKind::Session => true,
                            crate::memory::graph::NodeKind::Concept => false,
                        };
                        let folder = if is_session { "sessions" } else { "concepts" };
                        let filepath = std::path::PathBuf::from(&self.cfg.vault_path)
                            .join(folder)
                            .join(format!("{}.md", node_id));

                        if let Ok(content) = std::fs::read_to_string(filepath) {
                            self.active_modal = Some(ModalState::SessionViewer {
                                title: node.label.clone(),
                                content,
                                scroll: 0,
                                node_id,
                                is_session,
                            });
                            self.status = format!("Loaded {}", node.label);
                        } else {
                            self.status = format!("Failed to read {}", node.label);
                        }
                    }
                }
                _ => {}
            },
        }
        Ok(())
    }

    pub fn resume_session(&mut self, node_id: &str, content: &str) -> Result<()> {
        let mut messages = Vec::new();
        let mut in_transcript = false;
        let mut current_role = String::new();
        let mut current_content = String::new();

        for line in content.lines() {
            if line.starts_with("## Transcript") {
                in_transcript = true;
                continue;
            }

            if !in_transcript {
                continue;
            }

            if line.starts_with("**You**:") {
                if !current_role.is_empty() && !current_content.trim().is_empty() {
                    messages.push(ChatMessage {
                        role: current_role.clone(),
                        content: current_content.trim().to_string(),
                        images: None,
                    });
                    current_content.clear();
                }
                current_role = "user".to_string();
                current_content.push_str(line.trim_start_matches("**You**:").trim());
                current_content.push('\n');
            } else if line.starts_with("**Gemma**:") || line.starts_with("**QWEN**:") || line.starts_with("**assistant**:") {
                if !current_role.is_empty() && !current_content.trim().is_empty() {
                    messages.push(ChatMessage {
                        role: current_role.clone(),
                        content: current_content.trim().to_string(),
                        images: None,
                    });
                    current_content.clear();
                }
                current_role = "assistant".to_string();
                let prefix = if line.starts_with("**Gemma**:") {
                    "**Gemma**:"
                } else if line.starts_with("**QWEN**:") {
                    "**QWEN**:"
                } else {
                    "**assistant**:"
                };
                current_content.push_str(line.trim_start_matches(prefix).trim());
                current_content.push('\n');
            } else {
                if !current_role.is_empty() {
                    current_content.push_str(line);
                    current_content.push('\n');
                }
            }
        }

        if !current_role.is_empty() && !current_content.trim().is_empty() {
            messages.push(ChatMessage {
                role: current_role,
                content: current_content.trim().to_string(),
                images: None,
            });
        }

        if !messages.is_empty() {
            let system_prompt = self.messages.iter().find(|m| m.role == "system").cloned();
            self.messages.clear();
            if let Some(sys) = system_prompt {
                self.messages.push(sys);
            }
            self.messages.extend(messages);
            self.focus = Focus::Chat;
            self.scroll = 0;
            self.status = format!("Resumed session {}", node_id);
        } else {
            self.status = "Failed to parse transcript messages".into();
        }

        Ok(())
    }

    pub async fn handle_mouse(&mut self, mouse: MouseEvent) -> Result<()> {
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                match self.focus {
                    Focus::Chat => self.scroll += 3,
                    Focus::Graph => self.graph_scroll += 3,
                }
            }
            MouseEventKind::ScrollDown => {
                match self.focus {
                    Focus::Chat => self.scroll = self.scroll.saturating_sub(3),
                    Focus::Graph => self.graph_scroll = self.graph_scroll.saturating_sub(3),
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn submit(&mut self) -> Result<()> {
        let content = self.input.lines().join("\n").trim().to_string();
        if content.is_empty() || self.is_generating {
            return Ok(());
        }

        let mut new_input = TextArea::default();
        new_input.set_cursor_line_style(ratatui::style::Style::default());
        new_input.set_placeholder_text(" Type your message here... (Enter to submit, Shift+Enter for newline, Ctrl+V to paste)");
        self.input = new_input;

        // Enhancement #2: Concept RAG — inject relevant vault knowledge
        let concept_context = self.graph.build_concept_context(&content);
        if !concept_context.is_empty() {
            // Remove any previous RAG context messages
            self.messages.retain(|m| {
                !(m.role == "system" && m.content.starts_with("Relevant knowledge from your vault:"))
            });
            self.messages.push(ChatMessage {
                role: "system".into(),
                content: concept_context,
                images: None,
            });
        }

        // Resolve any image references (![alt](path)) into base64 data URIs
        let resolved_images = self.resolve_images_in_message(&content);
        let images = if resolved_images.is_empty() { None } else { Some(resolved_images) };

        self.messages.push(ChatMessage {
            role: "user".into(),
            content,
            images,
        });

        self.is_generating = true;
        self.current_response.clear();
        self.status = "Generating...".into();

        let (tx, rx) = mpsc::unbounded_channel();
        self.token_rx = Some(rx);

        self.backend.send_generate(self.messages.clone(), tx, self.thinking_mode);

        Ok(())
    }

    pub async fn quit(&mut self) -> Result<()> {
        if self.cfg.summarize_on_exit && self.messages.len() > 1 {
            let history: Vec<ChatMessage> = self.messages.iter()
                .filter(|m| m.role != "system")
                .cloned()
                .collect();

            if !history.is_empty() {
                let cfg = self.cfg.clone();
                std::thread::spawn(move || {
                    let mut b = crate::backend::process::Backend {
                        base_url: format!("http://127.0.0.1:{}", cfg.port),
                        child: None,
                        api_provider: cfg.api_provider.clone(),
                        api_key: crate::config::resolve_api_key(&cfg),
                        api_model: cfg.api_model.clone(),
                    };
                    if let Ok(summary) = summarize_session(&mut b, &history) {
                        if let Ok(vault) = VaultWriter::new(&cfg) {
                            let _ = vault.write_session(
                                &summary.summary,
                                &summary.concepts,
                                &summary.related,
                                &history,
                            );
                        }
                    }
                });
            }
        }

        self.should_quit = true;
        Ok(())
    }

    pub fn extract_code_blocks(&self) -> Vec<(String, String)> {
        let mut blocks = Vec::new();
        if let Some(msg) = self.messages.iter().filter(|m| m.role == "assistant").last() {
            let mut in_block = false;
            let mut current_lang = String::new();
            let mut current_code = String::new();

            for line in msg.content.lines() {
                if line.starts_with("```") {
                    if in_block {
                        blocks.push((current_lang.clone(), current_code.trim().to_string()));
                        current_lang.clear();
                        current_code.clear();
                        in_block = false;
                    } else {
                        current_lang = line.trim_start_matches("```").trim().to_string();
                        if current_lang.is_empty() {
                            current_lang = "Text".to_string();
                        }
                        in_block = true;
                    }
                } else if in_block {
                    current_code.push_str(line);
                    current_code.push('\n');
                }
            }
        }
        blocks
    }

    /// Save clipboard image data to the vault images directory and return the absolute path
    fn save_clipboard_image(&self, image: &arboard::ImageData) -> Result<String> {
        use image::RgbaImage;

        let images_dir = std::path::PathBuf::from(&self.cfg.vault_path).join("images");
        std::fs::create_dir_all(&images_dir)?;

        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
        let filename = format!("pasted_{}.png", timestamp);
        let filepath = images_dir.join(&filename);

        // Convert arboard image data to image crate format
        let img = RgbaImage::from_raw(
            image.width as u32,
            image.height as u32,
            image.bytes.to_vec(),
        ).ok_or_else(|| anyhow::anyhow!("Failed to create image from clipboard data"))?;

        // Downscale if larger than 1024x1024 to save context tokens
        let img = if img.width() > 1024 || img.height() > 1024 {
            let scale = 1024.0 / img.width().max(img.height()) as f32;
            let new_w = (img.width() as f32 * scale) as u32;
            let new_h = (img.height() as f32 * scale) as u32;
            image::imageops::resize(&img, new_w, new_h, image::imageops::FilterType::Lanczos3)
        } else {
            img
        };

        img.save(&filepath)?;

        Ok(filepath.to_string_lossy().to_string())
    }

    /// Scan message text for markdown image references ![...](path) and encode them as base64 data URIs
    fn resolve_images_in_message(&self, text: &str) -> Vec<String> {
        use base64::Engine;
        let mut images = Vec::new();

        // Simple regex-free parser for ![alt](path) patterns
        let mut pos = 0;
        while let Some(start) = text[pos..].find("![") {
            let abs_start = pos + start;
            if let Some(paren_open) = text[abs_start..].find("](") {
                let path_start = abs_start + paren_open + 2;
                if let Some(paren_close) = text[path_start..].find(')') {
                    let path = text[path_start..path_start + paren_close].trim();
                    let resolved = if std::path::Path::new(path).is_absolute() {
                        std::path::PathBuf::from(path)
                    } else {
                        std::path::PathBuf::from(&self.cfg.vault_path).join(path)
                    };

                    if resolved.exists() {
                        if let Ok(bytes) = std::fs::read(&resolved) {
                            let ext = resolved.extension()
                                .and_then(|e| e.to_str())
                                .unwrap_or("png")
                                .to_lowercase();
                            let mime = match ext.as_str() {
                                "jpg" | "jpeg" => "image/jpeg",
                                "gif" => "image/gif",
                                "webp" => "image/webp",
                                _ => "image/png",
                            };
                            let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                            images.push(format!("data:{};base64,{}", mime, b64));
                        }
                    }
                    pos = path_start + paren_close + 1;
                } else {
                    pos = abs_start + 2;
                }
            } else {
                pos = abs_start + 2;
            }
        }

        images
    }
}

fn build_context(graph: &MemoryGraph, max_nodes: usize) -> String {
    let nodes = graph.recent_nodes(max_nodes);
    if nodes.is_empty() {
        return String::new();
    }

    let mut ctx = String::from("You have the following memory from previous conversations:\n\n");
    for node in nodes {
        ctx.push_str(&format!(
            "- [{}] connected to: {}\n",
            node.label,
            node.connections.join(", ")
        ));
    }
    ctx.push_str("\nUse this context where relevant.");
    ctx
}