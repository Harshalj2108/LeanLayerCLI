import os

app_file = "src/tui/app.rs"
with open(app_file, "r", encoding="utf-8") as f:
    app_code = f.read()

# Replace ModalState
old_modal_state = """    CodeGatekeeper {
        request: crate::agent::executor::ExecutionRequest,
        pending_others: Vec<crate::agent::executor::ExecutionRequest>,
    },
}"""

new_modal_state = """    CodeGatekeeper {
        request: crate::agent::executor::ExecutionRequest,
        pending_others: Vec<crate::agent::executor::ExecutionRequest>,
    },
    ConfigEditor {
        active_field: usize,
        is_editing: bool,
        cfg_draft: crate::config::Config,
    },
}"""
app_code = app_code.replace(old_modal_state, new_modal_state)

# Add shortcut to toggle ConfigEditor (Ctrl+E)
old_shortcuts = """                            (KeyCode::Char('m'), KeyModifiers::CONTROL) => {
                                app.toggle_thinking_mode();
                            }
                            (KeyCode::Tab, _) => app.toggle_focus(),"""

new_shortcuts = """                            (KeyCode::Char('m'), KeyModifiers::CONTROL) => {
                                app.toggle_thinking_mode();
                            }
                            (KeyCode::Char('e'), KeyModifiers::CONTROL) => {
                                app.active_modal = Some(ModalState::ConfigEditor {
                                    active_field: 0,
                                    is_editing: false,
                                    cfg_draft: app.cfg.clone(),
                                });
                            }
                            (KeyCode::Tab, _) => app.toggle_focus(),"""
app_code = app_code.replace(old_shortcuts, new_shortcuts)

# Add logic for ConfigEditor in handle_key
old_handle = """                        _ => {}
                    }
                }
            }
            return Ok(());
        }"""

new_handle = """                        _ => {}
                    }
                }
                ModalState::ConfigEditor { active_field, is_editing, cfg_draft } => {
                    if *is_editing {
                        match key.code {
                            KeyCode::Esc => {
                                *is_editing = false;
                            }
                            KeyCode::Enter => {
                                let val = self.input.lines().join("");
                                match *active_field {
                                    0 => cfg_draft.model_path = val,
                                    1 => cfg_draft.vault_path = val,
                                    2 => cfg_draft.llama_server_path = if val.is_empty() { None } else { Some(val) },
                                    _ => {}
                                }
                                *is_editing = false;
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
                            }
                            KeyCode::Up => {
                                *active_field = active_field.saturating_sub(1);
                            }
                            KeyCode::Down => {
                                *active_field = (*active_field + 1).min(6);
                            }
                            KeyCode::Left => {
                                match *active_field {
                                    3 => cfg_draft.gpu_layers = cfg_draft.gpu_layers.saturating_sub(1),
                                    4 => cfg_draft.ctx_size = cfg_draft.ctx_size.saturating_sub(512),
                                    5 => cfg_draft.port = cfg_draft.port.saturating_sub(1),
                                    6 => cfg_draft.summarize_on_exit = !cfg_draft.summarize_on_exit,
                                    _ => {}
                                }
                            }
                            KeyCode::Right => {
                                match *active_field {
                                    3 => cfg_draft.gpu_layers += 1,
                                    4 => cfg_draft.ctx_size += 512,
                                    5 => cfg_draft.port += 1,
                                    6 => cfg_draft.summarize_on_exit = !cfg_draft.summarize_on_exit,
                                    _ => {}
                                }
                            }
                            KeyCode::Enter => {
                                if *active_field <= 2 {
                                    *is_editing = true;
                                    self.input = tui_textarea::TextArea::default();
                                    let current_val = match *active_field {
                                        0 => &cfg_draft.model_path,
                                        1 => &cfg_draft.vault_path,
                                        2 => cfg_draft.llama_server_path.as_deref().unwrap_or(""),
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
                                } else {
                                    self.status = "Failed to save configuration.".into();
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            return Ok(());
        }"""
app_code = app_code.replace(old_handle, new_handle)

with open(app_file, "w", encoding="utf-8") as f:
    f.write(app_code)


layout_file = "src/tui/layout.rs"
with open(layout_file, "r", encoding="utf-8") as f:
    layout_code = f.read()

# Add to draw_modal
old_draw_modal = """            f.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }).style(Style::default().fg(TEXT)), layout[0]);
            
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

new_draw_modal = """            f.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }).style(Style::default().fg(TEXT)), layout[0]);
            
            let footer = Line::from(vec![
                Span::styled(" y / Enter ", Style::default().fg(CRUST).bg(GREEN).add_modifier(Modifier::BOLD)),
                Span::raw(" Execute Code | "),
                Span::styled(" n / Esc ", Style::default().fg(CRUST).bg(RED).add_modifier(Modifier::BOLD)),
                Span::raw(" Deny"),
            ]);
            f.render_widget(Paragraph::new(footer).alignment(ratatui::layout::Alignment::Center).bg(SURFACE0), layout[1]);
        }
        super::app::ModalState::ConfigEditor { active_field, is_editing, cfg_draft } => {
            let block = Block::default()
                .title(Span::styled(" ⚙️ Settings & Configuration ", Style::default().fg(TEXT).add_modifier(Modifier::BOLD)))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(SAPPHIRE));

            let inner_area = block.inner(popup_area);
            f.render_widget(block, popup_area);

            let layout = Layout::default().direction(Direction::Vertical).constraints([Constraint::Min(0), Constraint::Length(3), Constraint::Length(1)]).split(inner_area);
            
            let fields = vec![
                ("Model Path", cfg_draft.model_path.clone()),
                ("Vault Path", cfg_draft.vault_path.clone()),
                ("Llama Server", cfg_draft.llama_server_path.clone().unwrap_or_else(|| "(bundled)".to_string())),
                ("GPU Layers", cfg_draft.gpu_layers.to_string()),
                ("Ctx Size", cfg_draft.ctx_size.to_string()),
                ("Port", cfg_draft.port.to_string()),
                ("Summarize on Exit", cfg_draft.summarize_on_exit.to_string()),
            ];

            let mut items = Vec::new();
            for (i, (name, val)) in fields.iter().enumerate() {
                let is_active = *active_field == i;
                let prefix = if is_active { " > " } else { "   " };
                let style = if is_active { Style::default().fg(GREEN).add_modifier(Modifier::BOLD) } else { Style::default().fg(TEXT) };
                
                let text = format!("{}{} : {}", prefix, name, val);
                items.push(ratatui::widgets::ListItem::new(text).style(style));
            }
            
            f.render_widget(ratatui::widgets::List::new(items), layout[0]);

            if *is_editing {
                let input_block = Block::default().title(" Edit Value ").borders(Borders::ALL).border_style(Style::default().fg(YELLOW));
                let mut input_clone = app.input.clone();
                input_clone.set_block(input_block);
                f.render_widget(&input_clone, layout[1]);
            } else {
                f.render_widget(Block::default().borders(Borders::NONE), layout[1]);
            }

            let footer = Line::from(vec![
                Span::styled(" Ctrl+S ", Style::default().fg(CRUST).bg(GREEN).add_modifier(Modifier::BOLD)),
                Span::raw(" Save | "),
                Span::styled(" Up/Down ", Style::default().fg(CRUST).bg(BLUE).add_modifier(Modifier::BOLD)),
                Span::raw(" Navigate | "),
                Span::styled(" Left/Right ", Style::default().fg(CRUST).bg(MAUVE).add_modifier(Modifier::BOLD)),
                Span::raw(" Adjust Num | "),
                Span::styled(" Esc ", Style::default().fg(CRUST).bg(RED).add_modifier(Modifier::BOLD)),
                Span::raw(" Cancel"),
            ]);
            f.render_widget(Paragraph::new(footer).alignment(ratatui::layout::Alignment::Center).bg(SURFACE0), layout[2]);
        }
    }
}"""
layout_code = layout_code.replace(old_draw_modal, new_draw_modal)

with open(layout_file, "w", encoding="utf-8") as f:
    f.write(layout_code)

print("Patch applied successfully.")
