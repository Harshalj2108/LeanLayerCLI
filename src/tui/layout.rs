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
        .split(f.area());

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
        for line in msg.content.lines() {
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
        for line in app.current_response.lines() {
            items.push(ListItem::new(Line::from(Span::raw(format!("   {}", line)))));
        }
    }

    let list = List::new(items);
    f.render_widget(list, inner);
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
    let status = Paragraph::new(Line::from(vec![
        Span::styled(" airllm ", Style::default().fg(Color::Black).bg(Color::Cyan)),
        Span::raw("  "),
        Span::styled(&app.status, Style::default().fg(Color::DarkGray)),
        Span::raw("   "),
        Span::styled("tab", Style::default().fg(Color::Cyan)),
        Span::raw(": switch panel  "),
        Span::styled("q", Style::default().fg(Color::Cyan)),
        Span::raw(": quit & save  "),
        Span::styled("↑↓", Style::default().fg(Color::Cyan)),
        Span::raw(": scroll"),
    ]));

    f.render_widget(status, area);
}