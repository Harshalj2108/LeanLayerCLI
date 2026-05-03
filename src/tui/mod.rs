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
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(cfg).await?;

    loop {
        terminal.draw(|f| layout::draw(f, &mut app))?;

        if event::poll(Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
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