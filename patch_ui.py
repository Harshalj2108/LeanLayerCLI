import os

app_file = "src/tui/app.rs"
with open(app_file, "r", encoding="utf-8") as f:
    app_code = f.read()

# Replace ModalState
old_modal_state = """#[derive(Clone)]
pub struct ModalState {
    pub title: String,
    pub content: String,
    pub scroll: usize,
    pub node_id: String,
    pub is_session: bool,
}"""

new_modal_state = """#[derive(Clone)]
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
}"""
app_code = app_code.replace(old_modal_state, new_modal_state)

# Replace tick Done
old_tick_done = """                    Ok(BackendMessage::Done) => {
                        self.messages.push(ChatMessage {
                            role: "assistant".into(),
                            content: self.current_response.clone(),
                        });
                        self.current_response.clear();
                        self.is_generating = false;
                        self.status = "Ready".into();
                        self.token_rx = None;
                        break;
                    }"""

new_tick_done = """                    Ok(BackendMessage::Done) => {
                        self.messages.push(ChatMessage {
                            role: "assistant".into(),
                            content: self.current_response.clone(),
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
                    }"""
app_code = app_code.replace(old_tick_done, new_tick_done)

# Replace handle_key modal logic
old_handle_modal = """        if let Some(modal) = &mut self.active_modal {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.active_modal = None;
                    self.status = "Ready".into();
                }
                KeyCode::Up => {
                    modal.scroll = modal.scroll.saturating_sub(1);
                }
                KeyCode::Down => {
                    modal.scroll += 1;
                }
                KeyCode::Char('r') if modal.is_session => {
                    let node_id = modal.node_id.clone();
                    let content = modal.content.clone();
                    self.active_modal = None;
                    self.resume_session(&node_id, &content)?;
                }
                _ => {}
            }
            return Ok(());
        }"""

new_handle_modal = """        if let Some(modal) = self.active_modal.clone() {
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
                                    match std::fs::read_to_string(path) {
                                        Ok(c) => Ok(crate::agent::executor::ExecutionStatus::Completed { stdout: c, stderr: "".into(), exit_code: 0 }),
                                        Err(e) => Ok(crate::agent::executor::ExecutionStatus::Failed(e.to_string())),
                                    }
                                }
                                crate::agent::tools::ToolCall::WriteFile { path, content } => {
                                    match std::fs::write(path, content) {
                                        Ok(_) => Ok(crate::agent::executor::ExecutionStatus::Completed { stdout: "File written successfully".into(), stderr: "".into(), exit_code: 0 }),
                                        Err(e) => Ok(crate::agent::executor::ExecutionStatus::Failed(e.to_string())),
                                    }
                                }
                                crate::agent::tools::ToolCall::SearchFiles { query, .. } => {
                                    Ok(crate::agent::executor::ExecutionStatus::Failed("Search not natively implemented".into()))
                                }
                            };
                            
                            let (out, success) = match result {
                                Ok(crate::agent::executor::ExecutionStatus::Completed { stdout, stderr, exit_code }) => {
                                    let mut s = stdout;
                                    if !stderr.is_empty() { s.push_str("\\n--- STDERR ---\\n"); s.push_str(&stderr); }
                                    (s, exit_code == 0)
                                }
                                Ok(crate::agent::executor::ExecutionStatus::Failed(e)) => (e, false),
                                Err(e) => (e.to_string(), false),
                                _ => ("Unknown error".into(), false),
                            };

                            self.messages.push(ChatMessage {
                                role: "user".into(),
                                content: crate::agent::tools::format_tool_result(&call, &out, success),
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
                                        content: format!("Execution result:\\nSTDOUT:\\n{}\\nSTDERR:\\n{}", stdout, stderr),
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
            }
            return Ok(());
        }"""
app_code = app_code.replace(old_handle_modal, new_handle_modal)

# Replace Graph active_modal
old_graph_modal = """                            self.active_modal = Some(ModalState {
                                title: node.label.clone(),
                                content,
                                scroll: 0,
                                node_id,
                                is_session,
                            });"""
new_graph_modal = """                            self.active_modal = Some(ModalState::SessionViewer {
                                title: node.label.clone(),
                                content,
                                scroll: 0,
                                node_id,
                                is_session,
                            });"""
app_code = app_code.replace(old_graph_modal, new_graph_modal)

with open(app_file, "w", encoding="utf-8") as f:
    f.write(app_code)


# Now layout.rs
layout_file = "src/tui/layout.rs"
with open(layout_file, "r", encoding="utf-8") as f:
    layout_code = f.read()

# Replace draw_modal
old_draw_modal = """fn draw_modal(f: &mut Frame, app: &App) {
    if let Some(modal) = &app.active_modal {
        let area = f.area();
        let popup_area = centered_rect(80, 85, area);

        f.render_widget(ratatui::widgets::Clear, popup_area);
        let clear_block = Block::default().bg(BASE);
        f.render_widget(clear_block, popup_area);

        let border_style = Style::default().fg(MAUVE);
        let block = Block::default()
            .title(Span::styled(
                format!(" 🔍 Historical Note: {} ", modal.title),
                Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(border_style);

        let inner_area = block.inner(popup_area);
        f.render_widget(block, popup_area);

        let modal_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(0),
                Constraint::Length(1),
            ])
            .split(inner_area);

        let text_area = modal_layout[0];
        let footer_area = modal_layout[1];

        let md_text = tui_markdown::from_str(&modal.content);
        let total_lines = md_text.lines.len();
        
        let scroll_pos = modal.scroll.min(total_lines.saturating_sub(text_area.height as usize));
        
        let para = Paragraph::new(md_text)
            .wrap(Wrap { trim: false })
            .scroll((scroll_pos as u16, 0))
            .style(Style::default().fg(TEXT));

        f.render_widget(para, text_area);

        let mut scrollbar_state = ScrollbarState::default()
            .content_length(total_lines)
            .position(scroll_pos);
        f.render_stateful_widget(
            Scrollbar::default()
                .orientation(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("↑"))
                .end_symbol(Some("↓"))
                .track_symbol(Some("│"))
                .thumb_symbol("█")
                .style(Style::default().fg(SURFACE1)),
            text_area,
            &mut scrollbar_state,
        );

        let mut footer_spans = vec![
            Span::styled(" Esc / q ", Style::default().fg(CRUST).bg(RED).add_modifier(Modifier::BOLD)),
            Span::raw(" Close | "),
            Span::styled(" Up / Down ", Style::default().fg(CRUST).bg(BLUE).add_modifier(Modifier::BOLD)),
            Span::raw(" Scroll"),
        ];

        if modal.is_session {
            footer_spans.push(Span::raw(" | "));
            footer_spans.push(Span::styled(" r ", Style::default().fg(CRUST).bg(GREEN).add_modifier(Modifier::BOLD)));
            footer_spans.push(Span::raw(" Resume Chat"));
        }

        let footer_text = Line::from(footer_spans);
        let footer_para = Paragraph::new(footer_text)
            .alignment(ratatui::layout::Alignment::Center)
            .bg(SURFACE0);
        f.render_widget(footer_para, footer_area);
    }
}"""

new_draw_modal = """fn draw_modal(f: &mut Frame, app: &App) {
    let modal = match &app.active_modal {
        Some(m) => m,
        None => return,
    };
    
    let area = f.area();
    let popup_area = centered_rect(80, 85, area);

    f.render_widget(ratatui::widgets::Clear, popup_area);
    let clear_block = Block::default().bg(BASE);
    f.render_widget(clear_block, popup_area);

    match modal {
        super::app::ModalState::SessionViewer { title, content, scroll, is_session, .. } => {
            let block = Block::default()
                .title(Span::styled(
                    format!(" 🔍 Historical Note: {} ", title),
                    Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(MAUVE));

            let inner_area = block.inner(popup_area);
            f.render_widget(block, popup_area);

            let modal_layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(1)])
                .split(inner_area);

            let md_text = tui_markdown::from_str(content);
            let total_lines = md_text.lines.len();
            let scroll_pos = (*scroll).min(total_lines.saturating_sub(modal_layout[0].height as usize));
            
            f.render_widget(
                Paragraph::new(md_text).wrap(Wrap { trim: false }).scroll((scroll_pos as u16, 0)).style(Style::default().fg(TEXT)),
                modal_layout[0]
            );

            let mut footer_spans = vec![
                Span::styled(" Esc / q ", Style::default().fg(CRUST).bg(RED).add_modifier(Modifier::BOLD)),
                Span::raw(" Close | "),
                Span::styled(" Up / Down ", Style::default().fg(CRUST).bg(BLUE).add_modifier(Modifier::BOLD)),
                Span::raw(" Scroll"),
            ];
            if *is_session {
                footer_spans.push(Span::raw(" | "));
                footer_spans.push(Span::styled(" r ", Style::default().fg(CRUST).bg(GREEN).add_modifier(Modifier::BOLD)));
                footer_spans.push(Span::raw(" Resume Chat"));
            }
            f.render_widget(Paragraph::new(Line::from(footer_spans)).alignment(ratatui::layout::Alignment::Center).bg(SURFACE0), modal_layout[1]);
        }
        super::app::ModalState::ToolGatekeeper { call, pending_others } => {
            let block = Block::default()
                .title(Span::styled(" 🛡️ Agentic Tool Execution Gatekeeper ", Style::default().fg(TEXT).add_modifier(Modifier::BOLD)))
                .borders(Borders::ALL)
                .border_type(BorderType::Double)
                .border_style(Style::default().fg(YELLOW));

            let inner_area = block.inner(popup_area);
            f.render_widget(block, popup_area);

            let layout = Layout::default().direction(Direction::Vertical).constraints([Constraint::Min(0), Constraint::Length(1)]).split(inner_area);
            
            let call_str = serde_json::to_string_pretty(call).unwrap_or_else(|_| "Parse error".into());
            let text = format!("The AI assistant wants to execute the following tool:\\n\\n{}\\n\\nPending actions in queue: {}", call_str, pending_others.len());
            
            f.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }).style(Style::default().fg(TEXT)), layout[0]);
            
            let footer = Line::from(vec![
                Span::styled(" y / Enter ", Style::default().fg(CRUST).bg(GREEN).add_modifier(Modifier::BOLD)),
                Span::raw(" Approve | "),
                Span::styled(" n / Esc ", Style::default().fg(CRUST).bg(RED).add_modifier(Modifier::BOLD)),
                Span::raw(" Deny"),
            ]);
            f.render_widget(Paragraph::new(footer).alignment(ratatui::layout::Alignment::Center).bg(SURFACE0), layout[1]);
        }
        super::app::ModalState::CodeGatekeeper { request, pending_others } => {
            let block = Block::default()
                .title(Span::styled(" ⚠️ Sandboxed Code Execution ", Style::default().fg(TEXT).add_modifier(Modifier::BOLD)))
                .borders(Borders::ALL)
                .border_type(BorderType::Double)
                .border_style(Style::default().fg(RED));

            let inner_area = block.inner(popup_area);
            f.render_widget(block, popup_area);

            let layout = Layout::default().direction(Direction::Vertical).constraints([Constraint::Min(0), Constraint::Length(1)]).split(inner_area);
            
            let text = format!("The AI assistant wants to execute {} code:\\n\\n{}\\n\\nPending blocks in queue: {}", request.language, request.code, pending_others.len());
            
            f.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }).style(Style::default().fg(TEXT)), layout[0]);
            
            let footer = Line::from(vec![
                Span::styled(" y / Enter ", Style::default().fg(CRUST).bg(GREEN).add_modifier(Modifier::BOLD)),
                Span::raw(" Execute Code | "),
                Span::styled(" n / Esc ", Style::default().fg(CRUST).bg(RED).add_modifier(Modifier::BOLD)),
                Span::raw(" Deny"),
            ]);
            f.render_widget(Paragraph::new(footer).alignment(ratatui::layout::Alignment::Center).bg(SURFACE0), layout[1]);
        }
    }
}"""

layout_code = layout_code.replace(old_draw_modal, new_draw_modal)

with open(layout_file, "w", encoding="utf-8") as f:
    f.write(layout_code)

print("Patch applied successfully.")
