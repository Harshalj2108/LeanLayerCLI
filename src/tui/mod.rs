pub mod app;
pub mod layout;
pub mod graph;

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::time::Duration;

use crate::config;
use app::App;

pub async fn run() -> Result<()> {
    let cfg = config::load()?;

    enable_raw_mode()?;
    let _ = crossterm::event::poll(std::time::Duration::from_millis(0));
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        crossterm::terminal::DisableLineWrap,
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(cfg).await?;
    let mut last_response_len = 0;

    loop {
        app.tick();

        let current_len = app.current_response.len() + app.messages.len();
        if current_len != last_response_len || !app.is_generating {
            terminal.draw(|f| layout::draw(f, &mut app))?;
            last_response_len = current_len;
        }

        if event::poll(Duration::from_millis(10))? {
            if let Event::Key(key) = event::read()? {
                while event::poll(Duration::from_millis(0))? {
                    let _ = event::read()?;
                }
                terminal.draw(|f| layout::draw(f, &mut app))?;
                last_response_len = app.current_response.len() + app.messages.len();
                match (key.code, key.modifiers) {
                    (KeyCode::Char('q'), KeyModifiers::NONE) => {
                        app.quit().await?;
                        break;
                    }
                    (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                        app.quit().await?;
                        break;
                    }
                    (KeyCode::Tab, _) => app.toggle_focus(),
                    _ => app.handle_key(key).await?,
                }
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
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}