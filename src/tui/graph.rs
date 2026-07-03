use ratatui::layout::Rect;
use ratatui::style::{Color, Style, Modifier};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, List, ListItem};
use ratatui::Frame;

use crate::tui::app::GraphNode;
use crate::memory::graph::{MemoryGraph, NodeKind};

pub fn render_graph(f: &mut Frame, area: Rect, nodes: &[GraphNode]) {
    let items = if nodes.is_empty() {
        vec![ListItem::new("No active context").style(Style::default().fg(Color::DarkGray))]
    } else {
        nodes
            .iter()
            .map(|node| {
                let color = match node.metadata.to_ascii_lowercase().as_str() {
                    "file" | "files" => Color::Cyan,
                    "function" | "functions" => Color::Magenta,
                    "thought" | "agentthought" => Color::Yellow,
                    _ => Color::Gray,
                };
                ListItem::new(Line::from(vec![
                    Span::styled(":: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(node.label.clone(), Style::default().fg(color).add_modifier(Modifier::BOLD)),
                    Span::raw(" "),
                    Span::styled(format!("[{}]", node.metadata), Style::default().fg(Color::DarkGray)),
                ]))
            })
            .collect()
    };

    f.render_widget(
        List::new(items).block(
            Block::default()
                .title(" Context Graph ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        ),
        area,
    );
}

/// Render a simple ASCII graph of recent memory nodes with selection highlighting
pub fn render_memory_graph_text<'a>(graph: &'a MemoryGraph, selected_index: Option<usize>, _width: usize, _height: usize) -> Text<'a> {
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

#[cfg(test)]
mod tests {
    use super::render_graph;
    use crate::tui::app::GraphNode;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn graph_renderer_maps_nodes_to_text() {
        let backend = TestBackend::new(40, 8);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let nodes = vec![GraphNode {
            label: "main".to_string(),
            metadata: "Function".to_string(),
        }];
        terminal
            .draw(|frame| render_graph(frame, frame.area(), &nodes))
            .expect("draw");
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("main"));
        assert!(rendered.contains("Context Graph"));
    }
}
