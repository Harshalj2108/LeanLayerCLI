use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    prelude::Stylize,
    widgets::{Block, Borders, BorderType, List, ListItem, Paragraph, Wrap, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use super::app::{App, Focus};
use super::graph::render_graph;

// Catppuccin Mocha Refined Color Palette
const CRUST: Color = Color::Rgb(17, 17, 27);      // App Status bar / Shell background
const MANTLE: Color = Color::Rgb(24, 24, 37);    // Sidebar panel background
const BASE: Color = Color::Rgb(30, 30, 46);      // Central Chat Workspace background
const SURFACE0: Color = Color::Rgb(49, 50, 68);  // Inactive elements / Badges
const SURFACE1: Color = Color::Rgb(69, 71, 90);  // Subtle Dividers
const TEXT: Color = Color::Rgb(205, 214, 244);      // Default Text
const SUBTEXT0: Color = Color::Rgb(166, 173, 200);  // Muted secondary text

const BLUE: Color = Color::Rgb(137, 180, 250);     // Active focus indicator
const MAUVE: Color = Color::Rgb(203, 166, 247);    // Assistant primary theme
const GREEN: Color = Color::Rgb(166, 227, 161);    // User response accent
const YELLOW: Color = Color::Rgb(249, 226, 175);   // Warning / Fast mode
const RED: Color = Color::Rgb(243, 139, 168);      // Deletions / Modal highlights
const SAPPHIRE: Color = Color::Rgb(116, 199, 236); // Context structures / Deep mode

pub fn draw(f: &mut Frame, app: &mut App) {
    // 3-tier classic terminal vertical hierarchy
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),    // Main workspace
            Constraint::Length(4), // Floating/Sleek input area
            Constraint::Length(1), // Informational status bar
        ])
        .split(f.area());

    let main_area = root[0];
    let input_area = root[1];
    let status_area = root[2];

    // Modern multi-panel setup: Split workspace into Workspace (Left) and Workspace Sidebar (Right)
    let panels = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(72), Constraint::Percentage(28)])
        .split(main_area);

    draw_chat(f, app, panels[0]);
    draw_graph(f, app, panels[1]);
    draw_input(f, app, input_area);
    draw_status(f, app, status_area);
    draw_modal(f, app);
}

fn draw_chat(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = matches!(app.focus, Focus::Chat);
    
    // Instead of heavy box borders, we use a top margin line with a clean unicode indicator
    let focus_marker = if is_focused { "●" } else { "○" };
    let focus_color = if is_focused { BLUE } else { SURFACE0 };

    let block = Block::default()
        .title(Line::from(vec![
            Span::styled(format!(" {} ", focus_marker), Style::default().fg(focus_color)),
            Span::styled("Chat Workspace", Style::default().fg(TEXT).add_modifier(Modifier::BOLD)),
        ]))
        .borders(Borders::TOP) // Remove surrounding borders, stick to clean top panel divider
        .border_style(Style::default().fg(SURFACE0))
        .bg(BASE);

    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut items: Vec<ListItem> = Vec::new();

    for msg in &app.messages {
        if msg.role == "system" {
            continue;
        }

        let (label, color) = match msg.role.as_str() {
            "user" => ("You", GREEN),
            "assistant" => ("AI Assistant", MAUVE),
            _ => ("System", YELLOW),
        };

        // Streamlined inline line metadata layout rather than chunky badges
        items.push(ListItem::new(Line::from(vec![
            Span::styled(format!("❯ {}", label), Style::default().fg(color).add_modifier(Modifier::BOLD)),
        ])));

        // Content indented elegantly
        let md_text = tui_markdown::from_str(&msg.content);
        items.push(ListItem::new(md_text));
        items.push(ListItem::new(Line::from(""))); // Spacing
    }

    // Streaming state response
    if !app.current_response.is_empty() {
        items.push(ListItem::new(Line::from(vec![
            Span::styled("❯ AI Assistant ", Style::default().fg(MAUVE).add_modifier(Modifier::BOLD)),
            Span::styled("▊", Style::default().fg(MAUVE).add_modifier(Modifier::RAPID_BLINK)),
        ])));
        let md_text = tui_markdown::from_str(&app.current_response);
        items.push(ListItem::new(md_text));
    }

    let list = List::new(items.clone()).style(Style::default().fg(TEXT));
    let mut state = ratatui::widgets::ListState::default();
    let total_items = items.len();
    
    let selected = if app.scroll > 0 {
        total_items.saturating_sub(1).saturating_sub(app.scroll)
    } else {
        total_items.saturating_sub(1)
    };
    state.select(Some(selected));
    
    f.render_stateful_widget(list, inner, &mut state);

    // Minimal single line scrollbar indicator on the edge
    let mut scrollbar_state = ScrollbarState::default()
        .content_length(total_items)
        .position(selected);
    f.render_stateful_widget(
        Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some("│"))
            .thumb_symbol("┃")
            .style(Style::default().fg(SURFACE0)),
        inner,
        &mut scrollbar_state,
    );
}

fn draw_graph(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = matches!(app.focus, Focus::Graph);
    let focus_marker = if is_focused { "●" } else { "○" };
    let focus_color = if is_focused { SAPPHIRE } else { SURFACE0 };

    // Sidebar structure with light left divider accent
    let block = Block::default()
        .title(Line::from(vec![
            Span::styled(format!(" {} ", focus_marker), Style::default().fg(focus_color)),
            Span::styled("Context Graph", Style::default().fg(TEXT).add_modifier(Modifier::BOLD)),
        ]))
        .borders(Borders::TOP | Borders::LEFT)
        .border_style(Style::default().fg(SURFACE0))
        .bg(MANTLE);

    let inner = block.inner(area);
    f.render_widget(block, area);

    let graph_text = render_graph(&app.graph, Some(app.selected_node_index), inner.width as usize, inner.height as usize);
    let total_lines = graph_text.lines.len();
    let scroll_pos = app.graph_scroll.min(total_lines.saturating_sub(inner.height as usize)) as u16;
    
    let para = Paragraph::new(graph_text)
        .wrap(Wrap { trim: false })
        .scroll((scroll_pos, 0))
        .style(Style::default().fg(SUBTEXT0));
        
    f.render_widget(para, inner);
}

fn draw_input(f: &mut Frame, app: &mut App, area: Rect) {
    let is_focused = matches!(app.focus, Focus::Chat);
    let border_color = if is_focused { GREEN } else { SURFACE0 };

    // Sleek continuous top horizontal line accent matching modern minimalist styles
    let block = Block::default()
        .title(Span::styled(" ✎ Compose Prompt ", Style::default().fg(TEXT).add_modifier(Modifier::BOLD)))
        .borders(Borders::TOP)
        .border_style(Style::default().fg(border_color))
        .bg(MANTLE);

    if app.is_generating {
        let input = Paragraph::new("  ● Syncing with AI cluster node...")
            .block(block)
            .style(Style::default().fg(SUBTEXT0).add_modifier(Modifier::ITALIC));
        f.render_widget(input, area);
    } else {
        app.input.set_block(block);
        app.input.set_style(Style::default().fg(TEXT));
        f.render_widget(&app.input, area);
    }
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let (mode_str, mode_color) = if app.thinking_mode {
        (" DEEP ENGINE ", BASE, SAPPHIRE)
    } else {
        (" FAST ENGINE ", BASE, YELLOW)
    };

    let status = Paragraph::new(Line::from(vec![
        Span::styled(" AIRLLM ", Style::default().fg(CRUST).bg(BLUE).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{}", mode_str), Style::default().fg(mode_color.0).bg(mode_color.1).add_modifier(Modifier::BOLD)),
        Span::raw(" ┃ "),
        Span::styled(&app.status, Style::default().fg(SUBTEXT0)),
        Span::raw("  "),
        Span::styled("Ctrl+V", Style::default().fg(BLUE)),
        Span::raw(" Paste ⚡ "),
        Span::styled("Ctrl+Y", Style::default().fg(BLUE)),
        Span::raw(" Yank ⚡ "),
        Span::styled("Ctrl+M", Style::default().fg(BLUE)),
        Span::raw(" Engine ⚡ "),
        Span::styled("Tab", Style::default().fg(BLUE)),
        Span::raw(" Shift Focus ⚡ "),
        Span::styled("Ctrl+Q", Style::default().fg(RED)),
        Span::raw(" Quit"),
    ])).bg(CRUST);

    f.render_widget(status, area);
}

fn draw_modal(f: &mut Frame, app: &App) {
    let modal = match &app.active_modal {
        Some(m) => m,
        None => return,
    };
    
    let area = f.area();
    let popup_area = centered_rect(75, 75, area);

    f.render_widget(ratatui::widgets::Clear, popup_area);
    let clear_block = Block::default().bg(BASE).borders(Borders::ALL).border_style(Style::default().fg(SURFACE1));
    f.render_widget(clear_block, popup_area);

    match modal {
        super::app::ModalState::SessionViewer { title, content, scroll, is_session, .. } => {
            let block = Block::default()
                .title(Span::styled(format!(" 📄 Historical Viewer: {} ", title), Style::default().fg(TEXT).add_modifier(Modifier::BOLD)))
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(SURFACE0));

            let inner_area = popup_area.inner(Margin { horizontal: 2, vertical: 1 });
            let modal_layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(1)])
                .split(inner_area);

            let md_text = tui_markdown::from_str(content);
            let total_lines = md_text.lines.len();
            let scroll_pos = (*scroll).min(total_lines.saturating_sub(modal_layout[0].height as usize));
            
            f.render_widget(Paragraph::new(md_text).wrap(Wrap { trim: false }).scroll((scroll_pos as u16, 0)), modal_layout[0]);

            let mut footer_spans = vec![
                Span::styled(" Esc ", Style::default().fg(TEXT).bg(SURFACE0)),
                Span::raw(" Close  "),
                Span::styled(" ▲/▼ ", Style::default().fg(TEXT).bg(SURFACE0)),
                Span::raw(" Navigate "),
            ];
            if *is_session {
                footer_spans.push(Span::styled(" R ", Style::default().fg(CRUST).bg(GREEN).add_modifier(Modifier::BOLD)));
                footer_spans.push(Span::raw(" Resume context"));
            }
            f.render_widget(Paragraph::new(Line::from(footer_spans)), modal_layout[1]);
        }
        super::app::ModalState::ToolGatekeeper { call, pending_others } => {
            let block = Block::default()
                .title(Span::styled(" ⚡ Pipeline Execution Request ", Style::default().fg(YELLOW).add_modifier(Modifier::BOLD)))
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(SURFACE0));

            let inner_area = popup_area.inner(Margin { horizontal: 2, vertical: 1 });
            let layout = Layout::default().direction(Direction::Vertical).constraints([Constraint::Min(0), Constraint::Length(1)]).split(inner_area);
            
            let call_str = serde_json::to_string_pretty(call).unwrap_or_else(|_| "Parse error".into());
            let text = format!("An automated agent component is requesting tool authorization:\n\n{}\n\nQueued operations pending: {}", call_str, pending_others.len());
            
            f.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), layout[0]);
            
            let footer = Line::from(vec![
                Span::styled(" Enter ", Style::default().fg(CRUST).bg(GREEN).add_modifier(Modifier::BOLD)),
                Span::raw(" Authorize  "),
                Span::styled(" Esc ", Style::default().fg(CRUST).bg(RED).add_modifier(Modifier::BOLD)),
                Span::raw(" Reject"),
            ]);
            f.render_widget(Paragraph::new(footer), layout[1]);
        }
        super::app::ModalState::CodeGatekeeper { request, pending_others } => {
            let block = Block::default()
                .title(Span::styled(" 🛠️ Sandboxed Script Block Execution ", Style::default().fg(RED).add_modifier(Modifier::BOLD)))
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(SURFACE0));

            let inner_area = popup_area.inner(Margin { horizontal: 2, vertical: 1 });
            let layout = Layout::default().direction(Direction::Vertical).constraints([Constraint::Min(0), Constraint::Length(1)]).split(inner_area);
            
            let text = format!("AI runtime environment requested local shell execution ({}) :\n\n{}\n\nPending actions: {}", request.language, request.code, pending_others.len());
            
            f.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), layout[0]);
            
            let footer = Line::from(vec![
                Span::styled(" Enter ", Style::default().fg(CRUST).bg(GREEN).add_modifier(Modifier::BOLD)),
                Span::raw(" Execute  "),
                Span::styled(" Esc ", Style::default().fg(CRUST).bg(RED).add_modifier(Modifier::BOLD)),
                Span::raw(" Terminate"),
            ]);
            f.render_widget(Paragraph::new(footer), layout[1]);
        }
        super::app::ModalState::ConfigEditor { active_field, is_editing, cfg_draft } => {
            let inner_area = popup_area.inner(Margin { horizontal: 2, vertical: 1 });
            let layout = Layout::default().direction(Direction::Vertical).constraints([Constraint::Min(0), Constraint::Length(3), Constraint::Length(1)]).split(inner_area);
            
            let fields = vec![
                ("Model Path", cfg_draft.model_path.clone()),
                ("Vault Path", cfg_draft.vault_path.clone()),
                ("Llama Server", cfg_draft.llama_server_path.clone().unwrap_or_else(|| "(bundled)".to_string())),
                ("GPU Layers", cfg_draft.gpu_layers.to_string()),
                ("Ctx Size", cfg_draft.ctx_size.to_string()),
                ("Port", cfg_draft.port.to_string()),
                ("Summarize on Exit", cfg_draft.summarize_on_exit.to_string()),
                ("API Provider", cfg_draft.api_provider.clone()),
                ("API Key", cfg_draft.api_key.clone().map(|k| if k.len() > 8 { format!("{}...{}", &k[..4], &k[k.len()-4..]) } else { "***".into() }).unwrap_or_else(|| "(env var)".into())),
                ("API Model", cfg_draft.api_model.clone().unwrap_or_else(|| "(default)".into())),
            ];

            let mut items = Vec::new();
            for (i, (name, val)) in fields.iter().enumerate() {
                let is_active = *active_field == i;
                let marker = if is_active { "❯ " } else { "  " };
                let style = if is_active { Style::default().fg(BLUE).add_modifier(Modifier::BOLD) } else { Style::default().fg(SUBTEXT0) };
                
                let text = format{}{}{} : {}", marker, name, val);
                items.push(ratatui::widgets::ListItem::new(text).style(style));
            }
            
            f.render_widget(ratatui::widgets::List::new(items), layout[0]);

            if *is_editing {
                let input_block = Block::default().title(" Field Edit Workspace ").borders(Borders::TOP).border_style(Style::default().fg(YELLOW));
                let mut input_clone = app.input.clone();
                input_clone.set_block(input_block);
                f.render_widget(&input_clone, layout[1]);
            }

            let footer = Line::from(vec![
                Span::styled(" Ctrl+S ", Style::default().fg(CRUST).bg(GREEN)),
                Span::raw(" Commit Changes ┃ "),
                Span::styled(" Esc ", Style::default().fg(TEXT).bg(SURFACE0)),
                Span::raw(" Exit Editor"),
            ]);
            f.render_widget(Paragraph::new(footer), layout[2]);
        }
        super::app::ModalState::CodeBlockYanker { blocks, selected_index, preview_scroll } => {
            let inner_area = popup_area.inner(Margin { horizontal: 2, vertical: 1 });
            let vertical_layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(1)])
                .split(inner_area);

            let horizontal_layout = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
                .split(vertical_layout[0]);

            let mut items = Vec::new();
            for (i, (lang, code)) in blocks.iter().enumerate() {
                let is_active = *selected_index == i;
                let marker = if is_active { "❯ " } else { "  " };
                let style = if is_active { Style::default().fg(GREEN).add_modifier(Modifier::BOLD) } else { Style::default().fg(SUBTEXT0) };

                let first_line = code.lines().next().unwrap_or("").trim();
                let preview = if first_line.len() > 14 { format!("{}...", &first_line[..12]) } else { first_line.to_string() };
                items.push(ListItem::new(format{}{}[{}] {}", marker, i + 1, lang, preview)).style(style));
            }

            f.render_widget(List::new(items), horizontal_layout[0]);

            if let Some((lang, code)) = blocks.get(*selected_index) {
                let lines: Vec<Line> = code.lines().map(|line| Line::from(line)).collect();
                let total_lines = lines.len();
                let scroll_pos = (*preview_scroll).min(total_lines.saturating_sub(horizontal_layout[1].height as usize));
                
                let paragraph = Paragraph::new(lines)
                    .wrap(Wrap { trim: false })
                    .scroll((scroll_pos as u16, 0));
                f.render_widget(paragraph, horizontal_layout[1]);
            }

            let footer = Line::from(vec![
                Span::styled(" Ctrl+Shift+C ", Style::default().fg(CRUST).bg(GREEN).add_modifier(Modifier::BOLD)),
                Span::raw(" Yank Block ┃ "),
                Span::styled(" Esc ", Style::default().fg(TEXT).bg(SURFACE0)),
                Span::raw(" Cancel"),
            ]);
            f.render_widget(Paragraph::new(footer), vertical_layout[1]);
        }
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

// Add tiny helper struct support mapping missing from original imports
use ratatui::layout::Margin;