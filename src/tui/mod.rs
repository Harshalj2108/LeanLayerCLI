pub mod app;
pub mod layout;
pub mod graph;

use anyhow::Result;
use ratatui::crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, EnableBracketedPaste, DisableBracketedPaste, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::time::{Duration, Instant};

use crate::config;
use app::App;

pub async fn run(start_config: bool) -> Result<()> {
    let cfg = config::load()?;

    enable_raw_mode()?;
    let _ = ratatui::crossterm::event::poll(std::time::Duration::from_millis(0));
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste,
        ratatui::crossterm::terminal::DisableLineWrap,
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(cfg).await?;
    
    if start_config {
        app.active_modal = Some(crate::tui::app::ModalState::ConfigEditor {
            active_field: 0,
            is_editing: false,
            cfg_draft: app.cfg.clone(),
        });
    }
    let mut last_response_len = 0;
    let mut last_draw = Instant::now();
    let frame_budget = Duration::from_millis(16); // ~60fps cap
    let rate_budget = Duration::from_secs(2);
    let mut needs_redraw = true;
    let mut last_rate_update = Instant::now();

    loop {
        app.tick().await;

        if last_rate_update.elapsed() >= rate_budget {
            app.update_rate_info().await;
            last_rate_update = Instant::now();
            needs_redraw = true;
        }

        // State-driven redraw detection
        let current_len = app.current_response.len() + app.messages.len();
        if current_len != last_response_len {
            needs_redraw = true;
            last_response_len = current_len;
        }

        // Throttled drawing — only redraw when state changed AND frame budget elapsed
        if needs_redraw && last_draw.elapsed() >= frame_budget {
            terminal.draw(|f| layout::draw(f, &mut app))?;
            last_draw = Instant::now();
            needs_redraw = false;
        }

        if event::poll(Duration::from_millis(10))? {
            match event::read()? {
                Event::Key(key) if key.kind != ratatui::crossterm::event::KeyEventKind::Release => {
                    if app.is_generating && key.code == KeyCode::Esc {
                        if let Some(task) = app.backend_task.take() {
                            task.abort();
                        }
                        app.is_generating = false;
                        app.status = "Cancelled".into();
                        app.token_rx = None;
                    } else if app.active_modal.is_some() {
                        app.handle_key(key).await?;
                    } else {
                        match (key.code, key.modifiers) {
                            // Enhancement #7: Modifier-gated quit (Ctrl+Q / Ctrl+C)
                            (KeyCode::Char('q'), KeyModifiers::CONTROL) => {
                                app.quit().await?;
                                break;
                            }
                            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                                app.quit().await?;
                                break;
                            }
                            // Mode cycle: Low → High → Ultra → Low (Ctrl+M)
                            (KeyCode::Char('m'), KeyModifiers::CONTROL) => {
                                app.cycle_mode();
                            }
                            // Agent Role cycle: Chat → Plan → Build → Chat (Ctrl+R)
                            (KeyCode::Char('r'), KeyModifiers::CONTROL) => {
                                app.cycle_role();
                            }
                            // Enhancement #4: Interactive Config Settings UI (Ctrl+E)
                            (KeyCode::Char('e'), KeyModifiers::CONTROL) => {
                                app.active_modal = Some(crate::tui::app::ModalState::ConfigEditor {
                                    active_field: 0,
                                    is_editing: false,
                                    cfg_draft: app.cfg.clone(),
                                });
                            }
                            // Phase 2.0: Workspace Panel (Ctrl+W)
                            (KeyCode::Char('w'), KeyModifiers::CONTROL) => {
                                app.open_workspace_panel();
                            }
                            // Phase 2.0: Git Status (Ctrl+G)
                            (KeyCode::Char('g'), KeyModifiers::CONTROL) => {
                                app.open_git_status();
                            }
                            // Fix: Use Ctrl+T to toggle focus instead of Tab, avoiding Alt+Tab phantom keystroke conflicts
                            (KeyCode::Char('t'), KeyModifiers::CONTROL) => app.toggle_focus(),
                            _ => app.handle_key(key).await?,
                        }
                    }
                    needs_redraw = true;
                }
                Event::Mouse(mouse) => {
                    app.handle_mouse(mouse).await?;
                    needs_redraw = true;
                }
                Event::Paste(text) => {
                    app.handle_paste(text);
                    needs_redraw = true;
                }
                _ => {}
            }
        }

        if app.should_quit {
            break;
        }
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        DisableBracketedPaste
    )?;
    terminal.show_cursor()?;

    Ok(())
}