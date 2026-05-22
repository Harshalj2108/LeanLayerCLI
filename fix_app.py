import os

app_file = "src/tui/app.rs"
with open(app_file, "r", encoding="utf-8") as f:
    app_code = f.read()

old_config_editor = """                ModalState::ConfigEditor { active_field, is_editing, cfg_draft } => {
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
                }"""

new_config_editor = """                ModalState::ConfigEditor { mut active_field, mut is_editing, mut cfg_draft } => {
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
                                active_field = (active_field + 1).min(6);
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
                                if active_field <= 2 {
                                    is_editing = true;
                                    self.input = tui_textarea::TextArea::default();
                                    let current_val = match active_field {
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
                                    return Ok(());
                                } else {
                                    self.status = "Failed to save configuration.".into();
                                }
                            }
                            _ => {}
                        }
                    }
                    self.active_modal = Some(ModalState::ConfigEditor { active_field, is_editing, cfg_draft });
                }"""

app_code = app_code.replace(old_config_editor, new_config_editor)

with open(app_file, "w", encoding="utf-8") as f:
    f.write(app_code)

print("Fix applied successfully.")
