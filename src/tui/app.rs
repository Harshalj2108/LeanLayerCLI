use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use tokio::sync::mpsc;

use crate::backend::{
    process::Backend,
    protocol::{BackendMessage, ChatMessage, FrontendMessage},
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

pub struct App {
    pub cfg: Config,
    pub messages: Vec<ChatMessage>,
    pub input: String,
    pub focus: Focus,
    pub is_generating: bool,
    pub current_response: String,
    pub status: String,
    pub should_quit: bool,
    pub graph: MemoryGraph,
    backend: Backend,
    pub scroll: usize,
}

impl App {
    pub async fn new(cfg: Config) -> Result<Self> {
        let status = "Loading model...".to_string();
        let backend = Backend::spawn(&cfg.backend_script, &cfg.model_path)?;
        let graph = MemoryGraph::load(&std::path::PathBuf::from(&cfg.vault_path))?;

        // Inject recent memory as system context
        let mut messages = Vec::new();
        let context = build_context(&graph, cfg.max_context_nodes);
        if !context.is_empty() {
            messages.push(ChatMessage {
                role: "system".into(),
                content: context,
            });
        }

        Ok(Self {
            cfg,
            messages,
            input: String::new(),
            focus: Focus::Chat,
            is_generating: false,
            current_response: String::new(),
            status: "Ready".into(),
            should_quit: false,
            graph,
            backend,
            scroll: 0,
        })
    }

    pub fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Chat => Focus::Graph,
            Focus::Graph => Focus::Chat,
        };
    }

    pub async fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        match self.focus {
            Focus::Chat => match key.code {
                KeyCode::Char(c) => self.input.push(c),
                KeyCode::Backspace => { self.input.pop(); }
                KeyCode::Enter => self.submit().await?,
                KeyCode::Up => self.scroll = self.scroll.saturating_sub(1),
                KeyCode::Down => self.scroll += 1,
                _ => {}
            },
            Focus::Graph => match key.code {
                KeyCode::Up => self.scroll = self.scroll.saturating_sub(1),
                KeyCode::Down => self.scroll += 1,
                _ => {}
            },
        }
        Ok(())
    }

    async fn submit(&mut self) -> Result<()> {
        let content = self.input.trim().to_string();
        if content.is_empty() || self.is_generating {
            return Ok(());
        }

        self.input.clear();
        self.messages.push(ChatMessage {
            role: "user".into(),
            content,
        });

        self.is_generating = true;
        self.current_response.clear();
        self.status = "Generating...".into();

        let (tx, mut rx) = mpsc::unbounded_channel();

        self.backend.send(&FrontendMessage::Generate {
            messages: self.messages.clone(),
        })?;

        // Stream tokens synchronously (backend is blocking I/O)
        // In a real async setup you'd offload this to a thread
        let tx_clone = tx.clone();
        self.backend.stream_response(&tx_clone)?;

        while let Ok(msg) = rx.try_recv() {
            match msg {
                BackendMessage::Token { content } => {
                    self.current_response.push_str(&content);
                }
                BackendMessage::Done => {
                    self.messages.push(ChatMessage {
                        role: "assistant".into(),
                        content: self.current_response.clone(),
                    });
                    self.current_response.clear();
                    self.is_generating = false;
                    self.status = "Ready".into();
                }
                BackendMessage::Error { message } => {
                    self.status = format!("Error: {}", message);
                    self.is_generating = false;
                }
                _ => {}
            }
        }

        Ok(())
    }

    pub async fn quit(&mut self) -> Result<()> {
        if self.cfg.summarize_on_exit && self.messages.len() > 1 {
            self.status = "Summarizing session...".into();

            // Filter out system messages for summarization
            let history: Vec<ChatMessage> = self.messages.iter()
                .filter(|m| m.role != "system")
                .cloned()
                .collect();

            if !history.is_empty() {
                match summarize_session(&mut self.backend, &history) {
                    Ok(summary) => {
                        let vault = VaultWriter::new(&self.cfg)?;
                        vault.write_session(
                            &summary.summary,
                            &summary.concepts,
                            &summary.related,
                            &history,
                        )?;
                    }
                    Err(e) => {
                        eprintln!("Failed to summarize: {}", e);
                    }
                }
            }
        }

        self.should_quit = true;
        Ok(())
    }
}

fn build_context(graph: &MemoryGraph, max_nodes: usize) -> String {
    let nodes = graph.recent_nodes(max_nodes);
    if nodes.is_empty() {
        return String::new();
    }

    let mut ctx = String::from("You have the following memory from previous conversations:\n\n");
    for node in nodes {
        ctx.push_str(&format!("- [{}] connected to: {}\n", node.label, node.connections.join(", ")));
    }
    ctx.push_str("\nUse this context where relevant.");
    ctx
}