use anyhow::Result;
use ratatui::crossterm::event::{KeyCode, KeyEvent, MouseEvent, MouseEventKind, KeyModifiers};
use tokio::sync::mpsc;
use tui_textarea::TextArea;
use arboard::Clipboard;

use crate::backend::{
    process::Backend,
    protocol::{BackendMessage, ChatMessage},
    ratelimit::RateLimiterHandle,
};
use crate::config::Config;
use crate::memory::{
    graph::MemoryGraph,
    summarize::summarize_session,
    vault::VaultWriter,
};
use crate::agent::workspace::{WorkspaceFile, GitStatus};

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
    WorkspacePanel {
        files: Vec<WorkspaceFile>,
        selected_index: usize,
        scroll: usize,
    },
    GitStatusModal {
        status: GitStatus,
        selected_index: usize,
        scroll: usize,
    },
    DiffReview {
        path: String,
        diff: String,
        proposed_content: String,
    },
    ModelSelection {
        models: Vec<(String, String)>, // (Provider, Model)
        selected_index: usize,
        scroll: usize,
    },
    ProviderSelection {
        providers: Vec<String>,
        selected_index: usize,
    },
    CommandAutocomplete {
        commands: Vec<String>,
        selected_index: usize,
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
    pub backend: Backend,
    pub scroll: usize,
    pub graph_scroll: usize,
    pub selected_node_index: usize,
    pub active_modal: Option<ModalState>,
    pub token_rx: Option<mpsc::UnboundedReceiver<BackendMessage>>,
    pub rate_limiter: RateLimiterHandle,
    pub rate_rpm: u32,
    pub rate_remaining: u32,
    pub rate_max: u32,
    pub agent_iteration_count: usize,
    pub token_count: usize,
    pub history_token_count: usize,
    bpe: Option<tiktoken_rs::CoreBPE>,
    clipboard: Option<Clipboard>,
    pub backend_task: Option<JoinHandle<()>>,
    pub pinned_files: std::collections::HashSet<String>,
    pub role: AgentRole,
}

impl<'a> App<'a> {
    pub async fn new(cfg: Config) -> Result<Self> {
        let backend = Backend::spawn(&cfg)?;
        let rate_limiter = backend.get_rate_limiter();
        let graph = MemoryGraph::load(&std::path::PathBuf::from(&cfg.vault_path))?;

        let mut messages = Vec::new();

        let mut mcp_clients = std::collections::HashMap::new();
        let mut mcp_tools = Vec::new();

        for (name, server_cfg) in &cfg.mcp_servers {
            if let Ok(client) = crate::agent::mcp::McpClient::spawn(name, server_cfg).await {
                if let Ok(tools) = client.list_tools().await {
                    for t in tools {
                        mcp_tools.push((name.clone(), t));
                    }
                }
                mcp_clients.insert(name.clone(), std::sync::Arc::new(tokio::sync::Mutex::new(client)));
            }
        }

        // Inject tool-calling system prompt
        let tool_prompt = crate::agent::tools::build_tool_system_prompt(crate::tui::app::AgentRole::Chat);
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

        let mut app = Self {
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
            rate_limiter,
            rate_rpm: 0,
            rate_remaining: 40,
            rate_max: 40,
            agent_iteration_count: 0,
            token_count: 0,
            history_token_count: 0,
            bpe: tiktoken_rs::cl100k_base().ok(),
            clipboard: Clipboard::new().ok(),
            backend_task: None,
            pinned_files: std::collections::HashSet::new(),
            role: AgentRole::Chat,
        })
    }

    pub fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Chat => Focus::Graph,
            Focus::Graph => Focus::Chat,
        };
    }

    pub fn cycle_mode(&mut self) {
        self.mode = self.mode.cycle();
    }

    pub fn cycle_role(&mut self) {
        self.role = self.role.cycle();
        self.status = format!("Agent role: {}", self.role.label().trim());

        let new_prompt = crate::agent::tools::build_tool_system_prompt(self.role);
        if let Some(sys) = self.messages.iter_mut().find(|m| m.role == "system") {
            sys.content = new_prompt;
        }
    }

    pub async fn update_rate_info(&mut self) {
        let provider = &self.cfg.api_provider;
        self.rate_rpm = self.rate_limiter.current_rpm(provider).await;
        self.rate_remaining = self.rate_limiter.remaining(provider).await;
        self.rate_max = self.rate_limiter.max_rpm();
    }

    pub fn open_workspace_panel(&mut self) {
        let root = std::path::PathBuf::from(&self.cfg.vault_path);
        match crate::agent::workspace::scan_workspace(&root, 5) {
            Ok(files) => {
                self.active_modal = Some(ModalState::WorkspacePanel {
                    files,
                    selected_index: 0,
                    scroll: 0,
                });
                self.status = "Workspace: Up/Down, Enter to read, R to refresh, Esc to close".into();
            }
            Err(e) => {
                self.status = format!("Failed to scan workspace: {}", e);
            }
        }
    }

    pub fn open_git_status(&mut self) {
        let root = std::path::PathBuf::from(&self.cfg.vault_path);
        if let Some(status) = crate::agent::workspace::get_git_status(&root) {
            let total_items = status.modified_files.len() + status.untracked_files.len();
            self.active_modal = Some(ModalState::GitStatusModal {
                status,
                selected_index: 0,
                scroll: 0,
            });
            self.status = format!("Git status ({} changes). Up/Down, Esc to close", total_items);
        } else {
            self.status = "Not a git repository or git not available".into();
        }
    }

    pub async fn tick(&mut self) {
        let mut needs_token_update = false;
        let mut needs_history_recalc = false;
        if let Some(rx) = &mut self.token_rx {
            loop {
                match rx.try_recv() {
                    Ok(BackendMessage::Token { content }) => {
                        self.current_response.push_str(&content);
                        needs_token_update = true;
                    }
                    Ok(BackendMessage::Done) => {
                        self.messages.push(ChatMessage {
                            role: "assistant".into(),
                            content: self.current_response.clone(),
                            images: None,
                        });
                        needs_history_recalc = true;
                        
                        let tool_calls = crate::agent::tools::parse_tool_calls(&self.current_response);
                        if !tool_calls.is_empty() {
                            let mut pending = tool_calls;
                            let first = pending.remove(0);
                            
                            if self.cfg.trust_level == "auto" {
                                self.status = "Auto-executing tool...".into();
                                let result = match &first {
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
                                    crate::agent::tools::ToolCall::SearchFiles { query, file_pattern } => {
                                        let root = std::path::PathBuf::from(&self.cfg.vault_path);
                                        crate::agent::executor::search_files(&root, query, file_pattern.as_deref(), 20)
                                    }
                                    crate::agent::tools::ToolCall::WebSearch { query } => {
                                        crate::agent::executor::execute_web_search(query).await
                                    }
                                    crate::agent::tools::ToolCall::ScrapeUrl { url } => {
                                        crate::agent::executor::execute_web_scrape(url).await
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
                                    content: crate::agent::tools::format_tool_result(&first, &out, success),
                                    images: None,
                                });
                                
                                self.agent_iteration_count += 1;
                                if self.agent_iteration_count > self.cfg.max_agent_iterations {
                                    self.messages.push(ChatMessage {
                                        role: "user".into(),
                                        content: "System: Maximum autonomous agent iterations reached. Awaiting user guidance.".into(),
                                        images: None,
                                    });
                                } else {
                                    if !pending.is_empty() {
                                        self.active_modal = Some(ModalState::ToolGatekeeper {
                                            call: pending.remove(0),
                                            pending_others: pending,
                                        });
                                    } else {
                                        let _ = self.trigger_backend();
                                        return; // We are generating again, don't clear token_rx
                                    }
                                }
                            } else {
                                self.active_modal = Some(ModalState::ToolGatekeeper {
                                    call: first,
                                    pending_others: pending,
                                });
                            }
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
                        if !self.current_response.is_empty() {
                            self.messages.push(ChatMessage {
                                role: "assistant".into(),
                                content: self.current_response.clone(),
                                images: None,
                            });
                            self.current_response.clear();
                        }
                        self.messages.push(ChatMessage {
                            role: "assistant".into(),
                            content: format!("⚠️ **API Error**:\n\n{}", message),
                            images: None,
                        });
                        needs_history_recalc = true;
                        self.is_generating = false;
                        self.status = "Ready".into();
                        self.token_rx = None;
                        break;
                    }
                    Ok(_) => {}
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                    Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                        self.is_generating = false;
                        self.token_rx = None;
                        break;
                    }
                }
            }
        }
        if needs_history_recalc {
            self.recalculate_history_tokens();
        }
        if needs_token_update {
            self.update_token_count();
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
                ModalState::ModelSelection { models, mut selected_index, mut scroll } => {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('q') => {
                            self.active_modal = None;
                            self.status = "Ready".into();
                        }
                        KeyCode::Up => {
                            selected_index = selected_index.saturating_sub(1);
                            if selected_index < scroll { scroll = selected_index; }
                            self.active_modal = Some(ModalState::ModelSelection { models, selected_index, scroll });
                        }
                        KeyCode::Down => {
                            if selected_index + 1 < models.len() {
                                selected_index += 1;
                                if selected_index >= scroll + 10 { scroll = selected_index - 9; }
                                self.active_modal = Some(ModalState::ModelSelection { models, selected_index, scroll });
                            }
                        }
                        KeyCode::Enter => {
                            let (provider, model) = models[selected_index].clone();
                            self.cfg.api_provider = provider.clone();
                            self.cfg.api_model = Some(model.clone());
                            let _ = crate::config::save(&self.cfg);
                            if let Err(e) = self.reload_backend() {
                                self.status = format!("Switched to {}, but: {}", provider, e);
                            } else {
                                self.status = format!("Switched to {} on {}", model, provider);
                            }
                            self.active_modal = None;
                        }
                        _ => {}
                    }
                }
                ModalState::ProviderSelection { providers, mut selected_index } => {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('q') => {
                            self.active_modal = None;
                        }
                        KeyCode::Up => {
                            selected_index = selected_index.saturating_sub(1);
                            self.active_modal = Some(ModalState::ProviderSelection { providers, selected_index });
                        }
                        KeyCode::Down => {
                            if selected_index + 1 < providers.len() {
                                selected_index += 1;
                                self.active_modal = Some(ModalState::ProviderSelection { providers, selected_index });
                            }
                        }
                        KeyCode::Enter => {
                            let provider = providers[selected_index].clone();
                            self.cfg.api_provider = provider.clone();
                            let _ = crate::config::save(&self.cfg);
                            if let Err(e) = self.reload_backend() {
                                self.status = format!("Provider set to {}, but: {}", provider, e);
                            } else {
                                self.status = format!("Provider set to {}", provider);
                            }
                            
                            // Automatically open input for API key entry
                            self.input = tui_textarea::TextArea::default();
                            self.input.insert_str(format!("/api_key "));
                            self.active_modal = None;
                        }
                        _ => {}
                    }
                }
                ModalState::CommandAutocomplete { commands, mut selected_index } => {
                    match key.code {
                        KeyCode::Esc => {
                            self.active_modal = None;
                        }
                        KeyCode::Up => {
                            selected_index = selected_index.saturating_sub(1);
                            self.active_modal = Some(ModalState::CommandAutocomplete { commands, selected_index });
                        }
                        KeyCode::Down => {
                            if selected_index + 1 < commands.len() {
                                selected_index += 1;
                                self.active_modal = Some(ModalState::CommandAutocomplete { commands, selected_index });
                            }
                        }
                        KeyCode::Enter => {
                            let cmd = commands[selected_index].clone();
                            self.active_modal = None;
                            self.input = tui_textarea::TextArea::default();
                            self.handle_slash_command(&cmd).await?;
                        }
                        _ => {
                            self.input.input(key);
                            let content = self.input.lines().join("\n");
                            if !content.starts_with('/') {
                                self.active_modal = None;
                            } else {
                                // Filter commands based on input
                                let all_cmds = vec!["/provider", "/model", "/api_key", "/trust", "/mcp add", "/mcp remove", "/config"];
                                let filtered: Vec<String> = all_cmds.into_iter()
                                    .filter(|c| c.starts_with(&content))
                                    .map(|s| s.to_string())
                                    .collect();
                                if filtered.is_empty() {
                                    self.active_modal = None;
                                } else {
                                    self.active_modal = Some(ModalState::CommandAutocomplete {
                                        commands: filtered,
                                        selected_index: 0,
                                    });
                                }
                            }
                        }
                    }
                    return Ok(()); // Handled input inside CommandAutocomplete
                }
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
                                let _ = self.trigger_backend();
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
                                    match crate::agent::executor::read_file_for_diff(path) {
                                        Ok(original) => {
                                            let diff = crate::agent::executor::generate_file_diff(path, original.as_deref(), content);
                                            self.active_modal = Some(ModalState::DiffReview {
                                                path: path.clone(),
                                                diff,
                                                proposed_content: content.clone(),
                                            });
                                            return Ok(());
                                        }
                                        Err(_) => {
                                            crate::agent::executor::write_file_global(path, content)
                                        }
                                    }
                                }
                                crate::agent::tools::ToolCall::SearchFiles { query, file_pattern } => {
                                    self.status = format!("Searching for: {}...", query);
                                    let root = std::path::PathBuf::from(&self.cfg.vault_path);                                    crate::agent::executor::search_files(
                                        &root,
                                        query,                                        file_pattern.as_deref(),                                        20,                                    )
                                }
                                crate::agent::tools::ToolCall::WebSearch { query } => {
                                    self.status = format!("Searching the web: {}...", query);
                                    crate::agent::executor::execute_web_search(query).await
                                }
                                crate::agent::tools::ToolCall::ScrapeUrl { url } => {
                                    self.status = format!("Scraping URL: {}...", url);
                                    crate::agent::executor::execute_web_scrape(url).await
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
                                let _ = self.trigger_backend();
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
                                    let _ = self.trigger_backend();
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
                ModalState::WorkspacePanel { files, selected_index, scroll } => {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('q') => {
                            self.active_modal = None;
                            self.status = "Ready".into();
                        }
                        KeyCode::Up => {
                            if let Some(ModalState::WorkspacePanel { selected_index: idx, .. }) = &mut self.active_modal {
                                *idx = idx.saturating_sub(1);
                            }
                        }
                        KeyCode::Down => {
                            if let Some(ModalState::WorkspacePanel { selected_index: idx, files, .. }) = &mut self.active_modal {
                                if !files.is_empty() {
                                    *idx = (*idx + 1).min(files.len() - 1);
                                }
                            }
                        }
                        KeyCode::Char('r') => {
                            // Refresh workspace scan
                            if let Some(ModalState::WorkspacePanel { files, selected_index, .. }) = &mut self.active_modal {
                                if let Ok(new_files) = crate::agent::workspace::scan_workspace(std::path::PathBuf::from(&self.cfg.vault_path).as_path(), 5) {
                                    *files = new_files;
                                    *selected_index = 0;
                                    self.status = "Workspace refreshed".into();
                                }
                            }
                        }
                        KeyCode::Enter => {
                            if let Some(ModalState::WorkspacePanel { files, selected_index, .. }) = &self.active_modal {
                                if let Some(file) = files.get(*selected_index) {
                                    if !file.is_dir {
                                        let path = file.relative_path.clone();
                                        if path.starts_with("sessions") || path.starts_with("concepts") {
                                            let full_path = std::path::PathBuf::from(&self.cfg.vault_path).join(&path);
                                            if let Ok(content) = std::fs::read_to_string(full_path) {
                                                let is_session = path.starts_with("sessions");
                                                let name = std::path::Path::new(&path).file_stem().unwrap_or_default().to_string_lossy().to_string();
                                                self.active_modal = Some(ModalState::SessionViewer {
                                                    title: name.clone(),
                                                    content,
                                                    scroll: 0,
                                                    node_id: name,
                                                    is_session,
                                                });
                                                self.status = format!("Loaded {}", path);
                                            }
                                        } else {
                                            let tool_call = crate::agent::tools::ToolCall::ReadFile { path };
                                            self.active_modal = Some(ModalState::ToolGatekeeper {
                                                call: tool_call,
                                                pending_others: Vec::new(),
                                            });
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                ModalState::GitStatusModal { status: git_status, selected_index, scroll } => {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('q') => {
                            self.active_modal = None;
                            self.status = "Ready".into();
                        }
                        KeyCode::Up => {
                            if let Some(ModalState::GitStatusModal { selected_index: idx, .. }) = &mut self.active_modal {
                                *idx = idx.saturating_sub(1);
                            }
                        }
                        KeyCode::Down => {
                            let total_items = if let Some(ModalState::GitStatusModal { status, .. }) = &self.active_modal {
                                status.modified_files.len() + status.untracked_files.len() + 1
                            } else { 0 };
                            if let Some(ModalState::GitStatusModal { selected_index: idx, .. }) = &mut self.active_modal {
                                if total_items > 0 {
                                    *idx = (*idx + 1).min(total_items - 1);
                                }
                            }
                        }
                        _ => {}
                    }
                }
                ModalState::DiffReview { path, diff, proposed_content } => {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('n') => {
                            let call = crate::agent::tools::ToolCall::WriteFile { path: path.clone(), content: proposed_content.clone() };
                            self.messages.push(ChatMessage {
                                role: "user".into(),
                                content: crate::agent::tools::format_tool_result(&call, "Write denied by user.", false),
                                images: None,
                            });
                            self.active_modal = None;
                            let _ = self.trigger_backend();
                        }
                        KeyCode::Enter | KeyCode::Char('y') => {
                            self.status = format!("Writing: {}...", path);
                            let result = crate::agent::executor::write_file_global(&path, &proposed_content);
                            let call = crate::agent::tools::ToolCall::WriteFile { path: path.clone(), content: String::new() };
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
                            self.active_modal = None;
                            let _ = self.trigger_backend();
                        }
                        _ => {}
                    }
                }
            }
            return Ok(());
        }

        match self.focus {
            Focus::Chat => {
                if key.code == KeyCode::Enter && key.modifiers.is_empty() {
                    self.submit().await?;
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
                    let content = self.input.lines().join("\n");
                    if content.starts_with('/') {
                        let all_cmds = vec!["/provider", "/model", "/api_key", "/trust", "/mcp add", "/mcp remove", "/config"];
                        let filtered: Vec<String> = all_cmds.into_iter()
                            .filter(|c| c.starts_with(&content))
                            .map(|s| s.to_string())
                            .collect();
                        if !filtered.is_empty() {
                            self.active_modal = Some(ModalState::CommandAutocomplete {
                                commands: filtered,
                                selected_index: 0,
                            });
                        }
                    }
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

                        if let Ok(content) = tokio::fs::read_to_string(filepath).await {
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

    async fn submit(&mut self) -> Result<()> {
        let content = self.input.lines().join("\n").trim().to_string();
        if content.is_empty() || self.is_generating {
            return Ok(());
        }

        if content.starts_with('/') {
            self.input = tui_textarea::TextArea::default();
            self.handle_slash_command(&content).await?;
            return Ok(());
        }

        let mut new_input = tui_textarea::TextArea::default();
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

        self.recalculate_history_tokens();
        self.agent_iteration_count = 0;
        self.update_token_count();
        self.trigger_backend()
    }

    pub fn recalculate_history_tokens(&mut self) {
        if let Some(bpe) = &self.bpe {
            let mut text = String::new();
            for msg in &self.messages {
                text.push_str(&msg.content);
                text.push('\n');
            }
            self.history_token_count = bpe.encode_ordinary(&text).len();
            self.token_count = self.history_token_count;
        }
    }

    pub fn update_token_count(&mut self) {
        if let Some(bpe) = &self.bpe {
            let current_len = if !self.current_response.is_empty() {
                bpe.encode_ordinary(&self.current_response).len()
            } else {
                0
            };
            self.token_count = self.history_token_count + current_len;
        }
    }

    pub fn reload_backend(&mut self) -> Result<()> {
        let new_backend = crate::backend::process::Backend::spawn(&self.cfg)?;
        self.rate_limiter = new_backend.get_rate_limiter();
        self.backend = new_backend;
        Ok(())
    }

    pub async fn handle_slash_command(&mut self, cmd: &str) -> Result<()> {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        if parts.is_empty() { return Ok(()); }
        
        match parts[0] {
            "/provider" => {
                let providers = vec![
                    "openai".to_string(),
                    "gemini".to_string(),
                    "anthropic".to_string(),
                    "openrouter".to_string(),
                    "nvidia".to_string(),
                    "local".to_string(),
                ];
                self.active_modal = Some(ModalState::ProviderSelection {
                    providers,
                    selected_index: 0,
                });
                self.status = "Select a provider (Up/Down, Enter to choose)".into();
            }
            "/model" => {
                let mut models: Vec<(String, String)> = Vec::new();
                let provider = self.cfg.api_provider.as_str();
                
                match provider {
                    "openai" => {
                        for m in ["gpt-4o", "gpt-4-turbo", "gpt-3.5-turbo", "gpt-4o-mini", "o1-mini", "o1-preview", "o3-mini"] {
                            models.push(("openai".into(), m.into()));
                        }
                    }
                    "gemini" => {
                        for m in ["gemini-1.5-pro", "gemini-1.5-flash", "gemini-2.5-pro", "gemini-2.5-flash", "gemini-2.0-flash", "gemini-2.0-pro-exp"] {
                            models.push(("gemini".into(), m.into()));
                        }
                    }
                    "anthropic" => {
                        for m in ["claude-3-5-sonnet-20241022", "claude-3-5-sonnet-20240620", "claude-3-opus-20240229", "claude-3-haiku-20240307", "claude-3-5-haiku-20241022"] {
                            models.push(("anthropic".into(), m.into()));
                        }
                    }
                    "nvidia" => {
                        for m in [
                            "meta/llama-3.3-70b-instruct", "meta/llama-3.1-70b-instruct", "meta/llama-3.1-8b-instruct",
                            "nvidia/llama-3.1-nemotron-70b-instruct", "nvidia/llama-3.1-nemotron-51b-instruct",
                            "nv-mistralai/mistral-nemo-12b-instruct", "mistralai/mixtral-8x22b-v0.1", "mistralai/mistral-large-2-instruct",
                            "microsoft/phi-3.5-moe-instruct"
                        ] {
                            models.push(("nvidia".into(), m.into()));
                        }
                    }
                    "openrouter" => {
                        for m in [
                            "anthropic/claude-3.5-sonnet", "openai/gpt-4o", "google/gemini-1.5-pro", 
                            "meta-llama/llama-3.1-405b-instruct", "meta-llama/llama-3.1-70b-instruct", 
                            "cohere/command-r-plus", "mistralai/mistral-large-2"
                        ] {
                            models.push(("openrouter".into(), m.into()));
                        }
                    }
                    _ => {
                        self.status = format!("No default models mapped for provider '{}'.", provider);
                        return Ok(());
                    }
                }
                
                self.active_modal = Some(ModalState::ModelSelection {
                    models,
                    selected_index: 0,
                    scroll: 0,
                });
                self.status = format!("Select model for {} (Up/Down, Enter to choose)", provider);
            }
            "/api_key" => {
                if parts.len() > 1 {
                    let key = parts[1].to_string();
                    self.cfg.api_keys.insert(self.cfg.api_provider.clone(), key);
                    let _ = crate::config::save(&self.cfg);
                    if let Err(e) = self.reload_backend() {
                        self.status = format!("API key saved, but backend failed: {}", e);
                    } else {
                        self.status = format!("API key updated and saved for provider '{}'.", self.cfg.api_provider);
                    }
                } else {
                    self.status = "Usage: /api_key <key>".into();
                }
            }
            "/trust" => {
                if parts.len() > 1 {
                    self.cfg.trust_level = parts[1].to_string();
                    crate::config::save(&self.cfg)?;
                    self.status = format!("Trust level set to {}", parts[1]);
                }
            }
            "/mcp" => {
                if parts.len() >= 4 && parts[1] == "add" {
                    let name = parts[2].to_string();
                    let command = parts[3].to_string();
                    let args = parts[4..].iter().map(|s| s.to_string()).collect();
                    
                    let server_cfg = crate::config::McpServerConfig {
                        command,
                        args,
                        env: std::collections::HashMap::new(),
                    };
                    self.cfg.mcp_servers.insert(name.clone(), server_cfg.clone());
                    crate::config::save(&self.cfg)?;
                    
                    if let Ok(client) = crate::agent::mcp::McpClient::spawn(&name, &server_cfg).await {
                        if let Ok(tools) = client.list_tools().await {
                            for t in tools {
                                self.mcp_tools.push((name.clone(), t));
                            }
                        }
                        self.mcp_clients.insert(name.clone(), std::sync::Arc::new(tokio::sync::Mutex::new(client)));
                        
                        let new_prompt = crate::agent::tools::build_tool_system_prompt(self.role, &self.mcp_tools);
                        if let Some(sys) = self.messages.iter_mut().find(|m| m.role == "system") {
                            sys.content = new_prompt;
                        }
                        self.status = format!("Added MCP server '{}'", name);
                    } else {
                        self.status = format!("Failed to connect to MCP server '{}'", name);
                    }
                } else if parts.len() >= 3 && parts[1] == "remove" {
                    let name = parts[2];
                    self.cfg.mcp_servers.remove(name);
                    crate::config::save(&self.cfg)?;
                    self.mcp_clients.remove(name);
                    self.mcp_tools.retain(|(s, _)| s != name);
                    let new_prompt = crate::agent::tools::build_tool_system_prompt(self.role, &self.mcp_tools);
                    if let Some(sys) = self.messages.iter_mut().find(|m| m.role == "system") {
                        sys.content = new_prompt;
                    }
                    self.status = format!("Removed MCP server '{}'", name);
                } else {
                    self.status = "Usage: /mcp add <name> <cmd> [args...] OR /mcp remove <name>".into();
                }
            }
            "/config" => {
                let toml_str = toml::to_string_pretty(&self.cfg).unwrap_or_else(|_| "Error".into());
                self.messages.push(ChatMessage {
                    role: "system".into(),
                    content: format!("**Current Configuration**:\n```toml\n{}\n```", toml_str),
                    images: None,
                });
                self.scroll = 0;
            }
            _ => {
                self.status = format!("Unknown command: {}", parts[0]);
            }
        }
        Ok(())
    }

    pub fn trigger_backend(&mut self) -> Result<()> {
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
                tokio::spawn(async move {
                    if let Ok(summary) = summarize_session(&history).await {
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