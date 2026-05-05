use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

use super::app::{App, Focus};
use super::graph::render_graph;

pub fn draw(f: &mut Frame, app: &mut App) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(f.size());

    let main_area = root[0];
    let input_area = root[1];
    let status_area = root[2];

    // Split main area: chat (left) + graph (right)
    let panels = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
        .split(main_area);

    draw_chat(f, app, panels[0]);
    draw_graph(f, app, panels[1]);
    draw_input(f, app, input_area);
    draw_status(f, app, status_area);
}

fn draw_chat(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = matches!(app.focus, Focus::Chat);
    let border_style = if is_focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default()
        .title(" 💬 Chat ")
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Build message lines
    let mut items: Vec<ListItem> = Vec::new();

    for msg in &app.messages {
        if msg.role == "system" {
            continue;
        }

        let (label, color) = match msg.role.as_str() {
            "user" => ("You", Color::Green),
            "assistant" => ("Gemma", Color::Magenta),
            _ => ("System", Color::Yellow),
        };

        // Header line
        items.push(ListItem::new(Line::from(vec![
            Span::styled(
                format!(" {} ", label),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
        ])));

        // Word-wrap content lines
        let wrap_width = inner.width.saturating_sub(4) as usize;
        let wrap_width = if wrap_width == 0 { 40 } else { wrap_width };
        let wrapped_lines = textwrap::wrap(&msg.content, wrap_width);
        for line in wrapped_lines {
            items.push(ListItem::new(Line::from(Span::raw(format!("   {}", line)))));
        }

        items.push(ListItem::new(Line::from(""))); // spacer
    }

    // Streaming response
    if !app.current_response.is_empty() {
        items.push(ListItem::new(Line::from(vec![
            Span::styled(
                " Gemma ",
                Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
            ),
            Span::styled("▍", Style::default().fg(Color::Magenta)),
        ])));
        let wrap_width = inner.width.saturating_sub(4) as usize;
        let wrap_width = if wrap_width == 0 { 40 } else { wrap_width };
        let wrapped_lines = textwrap::wrap(&app.current_response, wrap_width);
        for line in wrapped_lines {
            items.push(ListItem::new(Line::from(Span::raw(format!("   {}", line)))));
        }
    }

    let list = List::new(items.clone());
    let mut state = ratatui::widgets::ListState::default();
    let total_items = items.len();
    if total_items > 0 {
        let selected = total_items.saturating_sub(1).saturating_sub(app.scroll);
        state.select(Some(selected));
    }
    f.render_stateful_widget(list, inner, &mut state);
}

fn draw_graph(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = matches!(app.focus, Focus::Graph);
    let border_style = if is_focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default()
        .title(" 🧠 Memory ")
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    f.render_widget(block, area);

    let graph_text = render_graph(&app.graph, inner.width as usize, inner.height as usize);
    let para = Paragraph::new(graph_text).wrap(Wrap { trim: false });
    f.render_widget(para, inner);
}

fn draw_input(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = matches!(app.focus, Focus::Chat);
    let border_style = if is_focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let display = if app.is_generating {
        "  Generating...".to_string()
    } else {
        format!("  {}_", app.input)
    };

    let input = Paragraph::new(display)
        .block(
            Block::default()
                .title(" Input ")
                .borders(Borders::ALL)
                .border_style(border_style),
        )
        .style(Style::default().fg(Color::White));

    f.render_widget(input, area);
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let (mode_str, mode_color) = if app.thinking_mode {
        ("[DEEP]", Color::Cyan)
    } else {
        ("[FAST]", Color::Yellow)
    };

    let status = Paragraph::new(Line::from(vec![
        Span::styled(" airllm ", Style::default().fg(Color::Black).bg(Color::Cyan)),
        Span::raw("  "),
        Span::styled(&app.status, Style::default().fg(Color::DarkGray)),
        Span::raw("   "),
        Span::styled(mode_str, Style::default().fg(mode_color)),
        Span::raw("   "),
        Span::styled("m", Style::default().fg(Color::Cyan)),
        Span::raw(": toggle mode   "),
        Span::styled("tab", Style::default().fg(Color::Cyan)),
        Span::raw(": switch   "),
        Span::styled("q", Style::default().fg(Color::Cyan)),
        Span::raw(": quit"),
    ]));

    f.render_widget(status, area);
}