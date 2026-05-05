use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, MouseEvent, MouseEventKind};
use tokio::sync::mpsc;

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
    pub thinking_mode: bool,
    backend: Backend,
    pub scroll: usize,
    pub token_rx: Option<mpsc::UnboundedReceiver<BackendMessage>>,
}

impl App {
    pub async fn new(cfg: Config) -> Result<Self> {
        let backend = Backend::spawn(&cfg.backend_script, &cfg.model_path)?;
        let graph = MemoryGraph::load(&std::path::PathBuf::from(&cfg.vault_path))?;

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
            thinking_mode: false,
            backend,
            scroll: 0,
            token_rx: None,
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
                        });
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

    pub async fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        match self.focus {
            Focus::Chat => match key.code {
                KeyCode::Char(c) => self.input.push(c),
                KeyCode::Backspace => { self.input.pop(); }
                KeyCode::Enter => self.submit()?,
                KeyCode::Up => self.scroll += 1,
                KeyCode::Down => self.scroll = self.scroll.saturating_sub(1),
                _ => {}
            },
            Focus::Graph => match key.code {
                KeyCode::Up => self.scroll += 1,
                KeyCode::Down => self.scroll = self.scroll.saturating_sub(1),
                _ => {}
            },
        }
        Ok(())
    }

    pub async fn handle_mouse(&mut self, mouse: MouseEvent) -> Result<()> {
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                // Scroll wheel up = move viewport to older messages
                self.scroll += 3;
            }
            MouseEventKind::ScrollDown => {
                // Scroll wheel down = move viewport to newer messages
                self.scroll = self.scroll.saturating_sub(3);
            }
            _ => {}
        }
        Ok(())
    }

    fn submit(&mut self) -> Result<()> {
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
                        base_url: "http://127.0.0.1:8081".into(),
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