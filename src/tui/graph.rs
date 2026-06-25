use ratatui::text::{Line, Span, Text};
use ratatui::style::{Color, Style, Modifier};

use crate::memory::graph::{MemoryGraph, NodeKind};

/// Render a simple ASCII graph of recent memory nodes with selection highlighting
pub fn render_graph<'a>(graph: &'a MemoryGraph, selected_index: Option<usize>, _width: usize, _height: usize) -> Text<'a> {
    let nodes = graph.recent_nodes(8);

    if nodes.is_empty() {
        return Text::from(vec![
            Line::from(""),
            Line::from(Span::styled(
                "  No memories yet.",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "  Start chatting — sessions",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(Span::styled(
                "  will appear here as nodes.",
                Style::default().fg(Color::DarkGray),
            )),
        ]);
    }

    let mut lines: Vec<Line> = vec![Line::from("")];

    for (i, node) in nodes.iter().enumerate() {
        let (icon, color) = match node.kind {
            NodeKind::Session => ("◉", Color::Cyan),
            NodeKind::Concept => ("◈", Color::Magenta),
        };

        let label = if node.label.len() > 18 {
            format!("{}…", &node.label[..17])
        } else {
            node.label.clone()
        };

        let is_selected = selected_index == Some(i);
        let prefix = if is_selected { "▶ " } else { "  " };
        let label_style = if is_selected {
            Style::default().fg(color).add_modifier(Modifier::BOLD).bg(Color::Rgb(49, 50, 68))
        } else {
            Style::default().fg(color)
        };

        lines.push(Line::from(vec![
            Span::styled(prefix, Style::default().fg(Color::Rgb(249, 226, 175)).add_modifier(Modifier::BOLD)),
            Span::styled(icon, Style::default().fg(color).add_modifier(Modifier::BOLD)),
            Span::raw(" "),
            Span::styled(label, label_style),
        ]));

        // Show connections for this node (indented)
        for conn in node.connections.iter().take(3) {
            let short_conn = if conn.len() > 14 {
                format!("{}…", &conn[..13])
            } else {
                conn.clone()
            };

            let connector = if i < nodes.len() - 1 { "  │  └─" } else { "     └─" };
            lines.push(Line::from(vec![
                Span::styled(connector, Style::default().fg(Color::DarkGray)),
                Span::raw(" "),
                Span::styled(short_conn, Style::default().fg(Color::Gray)),
            ]));
        }

        if !node.connections.is_empty() {
            lines.push(Line::from(""));
        }
    }

    Text::from(lines)
}