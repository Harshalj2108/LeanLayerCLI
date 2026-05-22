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

// Catppuccin Mocha Color Palette
const CRUST: Color = Color::Rgb(17, 17, 27);
const MANTLE: Color = Color::Rgb(24, 24, 37);
const BASE: Color = Color::Rgb(30, 30, 46);
const SURFACE0: Color = Color::Rgb(49, 50, 68);
const SURFACE1: Color = Color::Rgb(69, 71, 90);
const TEXT: Color = Color::Rgb(205, 214, 244);
const SUBTEXT0: Color = Color::Rgb(166, 173, 200);

const BLUE: Color = Color::Rgb(137, 180, 250);
const MAUVE: Color = Color::Rgb(203, 166, 247);
const GREEN: Color = Color::Rgb(166, 227, 161);
const YELLOW: Color = Color::Rgb(249, 226, 175);
const RED: Color = Color::Rgb(243, 139, 168);
const SAPPHIRE: Color = Color::Rgb(116, 199, 236);

pub fn draw(f: &mut Frame, app: &mut App) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(5), // Multi-line input space
            Constraint::Length(1),
        ])
        .split(f.area());

    let main_area = root[0];
    let input_area = root[1];
    let status_area = root[2];

    // Split main area: chat (left) + graph (right)
    let panels = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(main_area);

    draw_chat(f, app, panels[0]);
    draw_graph(f, app, panels[1]);
    draw_input(f, app, input_area);
    draw_status(f, app, status_area);
    draw_modal(f, app);
}

fn draw_chat(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = matches!(app.focus, Focus::Chat);
    let border_style = if is_focused {
        Style::default().fg(BLUE)
    } else {
        Style::default().fg(SURFACE1)
    };

    let block = Block::default()
        .title(Span::styled(" 💬 Chat ", Style::default().fg(TEXT).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style)
        .bg(BASE);

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Build message lines
    let mut items: Vec<ListItem> = Vec::new();

    for msg in &app.messages {
        if msg.role == "system" {
            continue;
        }

        let (label, color) = match msg.role.as_str() {
            "user" => ("You", GREEN),
            "assistant" => ("QWEN", MAUVE),
            _ => ("System", YELLOW),
        };

        // Header line
        items.push(ListItem::new(Line::from(vec![
            Span::styled(
                format!(" {} ", label),
                Style::default().fg(color).bg(SURFACE0).add_modifier(Modifier::BOLD),
            ),
        ])));

        // Markdown content
        let md_text = tui_markdown::from_str(&msg.content);
        items.push(ListItem::new(md_text));

        items.push(ListItem::new(Line::from(""))); // spacer
    }

    // Streaming response
    if !app.current_response.is_empty() {
        items.push(ListItem::new(Line::from(vec![
            Span::styled(
                " QWEN ",
                Style::default().fg(MAUVE).bg(SURFACE0).add_modifier(Modifier::BOLD),
            ),
            Span::styled("▍", Style::default().fg(MAUVE).add_modifier(Modifier::RAPID_BLINK)),
        ])));
        let md_text = tui_markdown::from_str(&app.current_response);
        items.push(ListItem::new(md_text));
    }

    let list = List::new(items.clone()).style(Style::default().fg(TEXT));
    let mut state = ratatui::widgets::ListState::default();
    let total_items = items.len();
    
    // Auto-scroll logic with manual scroll offset
    let _max_scroll = total_items.saturating_sub(inner.height as usize);
    let selected = if app.scroll > 0 {
        total_items.saturating_sub(1).saturating_sub(app.scroll)
    } else {
        total_items.saturating_sub(1)
    };
    state.select(Some(selected));
    
    f.render_stateful_widget(list, inner, &mut state);

    // Render Scrollbar
    let mut scrollbar_state = ScrollbarState::default()
        .content_length(total_items)
        .position(selected);
    f.render_stateful_widget(
        Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓"))
            .track_symbol(Some("│"))
            .thumb_symbol("█")
            .style(Style::default().fg(SURFACE1)),
        inner,
        &mut scrollbar_state,
    );
}

fn draw_graph(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = matches!(app.focus, Focus::Graph);
    let border_style = if is_focused {
        Style::default().fg(SAPPHIRE)
    } else {
        Style::default().fg(SURFACE1)
    };

    let block = Block::default()
        .title(Span::styled(" 🧠 Memory ", Style::default().fg(TEXT).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style)
        .bg(BASE);

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

    // Render Scrollbar
    let mut scrollbar_state = ScrollbarState::default()
        .content_length(total_lines)
        .position(scroll_pos as usize);
    f.render_stateful_widget(
        Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓"))
            .track_symbol(Some("│"))
            .thumb_symbol("█")
            .style(Style::default().fg(SURFACE1)),
        inner,
        &mut scrollbar_state,
    );
}

fn draw_input(f: &mut Frame, app: &mut App, area: Rect) {
    let is_focused = matches!(app.focus, Focus::Chat);
    let border_style = if is_focused {
        Style::default().fg(GREEN)
    } else {
        Style::default().fg(SURFACE1)
    };

    let block = Block::default()
        .title(Span::styled(" Input ", Style::default().fg(TEXT).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style)
        .bg(MANTLE);

    if app.is_generating {
        let display = "  Generating...";
        let input = Paragraph::new(display)
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
        ("[DEEP]", SAPPHIRE)
    } else {
        ("[FAST]", YELLOW)
    };

    let status = Paragraph::new(Line::from(vec![
        Span::styled(" airllm ", Style::default().fg(CRUST).bg(BLUE).add_modifier(Modifier::BOLD)),
        Span::raw(" "),
        Span::styled(&app.status, Style::default().fg(SUBTEXT0)),
        Span::raw(" | "),
        Span::styled(mode_str, Style::default().fg(mode_color).add_modifier(Modifier::BOLD)),
        Span::raw(" | "),
        Span::styled("^V", Style::default().fg(BLUE)),
        Span::raw(": Paste | "),
        Span::styled("^Y", Style::default().fg(BLUE)),
        Span::raw(": Yank | "),
        Span::styled("^M", Style::default().fg(BLUE)),
        Span::raw(": Mode | "),
        Span::styled("Tab", Style::default().fg(BLUE)),
        Span::raw(": Switch | "),
        Span::styled("^Q", Style::default().fg(RED)),
        Span::raw(": Quit"),
    ])).bg(CRUST);

    f.render_widget(status, area);
}

fn draw_modal(f: &mut Frame, app: &App) {
    let modal = match &app.active_modal {
        Some(m) => m,
        None => return,
    };
    
    let area = f.area();
    let popup_area = centered_rect(80, 85, area);

    f.render_widget(ratatui::widgets::Clear, popup_area);
    let clear_block = Block::default().bg(BASE);
    f.render_widget(clear_block, popup_area);

    match modal {
        super::app::ModalState::SessionViewer { title, content, scroll, is_session, .. } => {
            let block = Block::default()
                .title(Span::styled(
                    format!(" 🔍 Historical Note: {} ", title),
                    Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(MAUVE));

            let inner_area = block.inner(popup_area);
            f.render_widget(block, popup_area);

            let modal_layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(1)])
                .split(inner_area);

            let md_text = tui_markdown::from_str(content);
            let total_lines = md_text.lines.len();
            let scroll_pos = (*scroll).min(total_lines.saturating_sub(modal_layout[0].height as usize));
            
            f.render_widget(
                Paragraph::new(md_text).wrap(Wrap { trim: false }).scroll((scroll_pos as u16, 0)).style(Style::default().fg(TEXT)),
                modal_layout[0]
            );

            let mut footer_spans = vec![
                Span::styled(" Esc / q ", Style::default().fg(CRUST).bg(RED).add_modifier(Modifier::BOLD)),
                Span::raw(" Close | "),
                Span::styled(" Up / Down ", Style::default().fg(CRUST).bg(BLUE).add_modifier(Modifier::BOLD)),
                Span::raw(" Scroll"),
            ];
            if *is_session {
                footer_spans.push(Span::raw(" | "));
                footer_spans.push(Span::styled(" r ", Style::default().fg(CRUST).bg(GREEN).add_modifier(Modifier::BOLD)));
                footer_spans.push(Span::raw(" Resume Chat"));
            }
            f.render_widget(Paragraph::new(Line::from(footer_spans)).alignment(ratatui::layout::Alignment::Center).bg(SURFACE0), modal_layout[1]);
        }
        super::app::ModalState::ToolGatekeeper { call, pending_others } => {
            let block = Block::default()
                .title(Span::styled(" 🛡️ Agentic Tool Execution Gatekeeper ", Style::default().fg(TEXT).add_modifier(Modifier::BOLD)))
                .borders(Borders::ALL)
                .border_type(BorderType::Double)
                .border_style(Style::default().fg(YELLOW));

            let inner_area = block.inner(popup_area);
            f.render_widget(block, popup_area);

            let layout = Layout::default().direction(Direction::Vertical).constraints([Constraint::Min(0), Constraint::Length(1)]).split(inner_area);
            
            let call_str = serde_json::to_string_pretty(call).unwrap_or_else(|_| "Parse error".into());
            let text = format!("The AI assistant wants to execute the following tool:\n\n{}\n\nPending actions in queue: {}", call_str, pending_others.len());
            
            f.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }).style(Style::default().fg(TEXT)), layout[0]);
            
            let footer = Line::from(vec![
                Span::styled(" y / Enter ", Style::default().fg(CRUST).bg(GREEN).add_modifier(Modifier::BOLD)),
                Span::raw(" Approve | "),
                Span::styled(" n / Esc ", Style::default().fg(CRUST).bg(RED).add_modifier(Modifier::BOLD)),
                Span::raw(" Deny"),
            ]);
            f.render_widget(Paragraph::new(footer).alignment(ratatui::layout::Alignment::Center).bg(SURFACE0), layout[1]);
        }
        super::app::ModalState::CodeGatekeeper { request, pending_others } => {
            let block = Block::default()
                .title(Span::styled(" ⚠️ Sandboxed Code Execution ", Style::default().fg(TEXT).add_modifier(Modifier::BOLD)))
                .borders(Borders::ALL)
                .border_type(BorderType::Double)
                .border_style(Style::default().fg(RED));

            let inner_area = block.inner(popup_area);
            f.render_widget(block, popup_area);

            let layout = Layout::default().direction(Direction::Vertical).constraints([Constraint::Min(0), Constraint::Length(1)]).split(inner_area);
            
            let text = format!("The AI assistant wants to execute {} code:\n\n{}\n\nPending blocks in queue: {}", request.language, request.code, pending_others.len());
            
            f.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }).style(Style::default().fg(TEXT)), layout[0]);
            
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
                ("API Provider", cfg_draft.api_provider.clone()),
                ("API Key", cfg_draft.api_key.clone().map(|k| if k.len() > 8 { format!("{}...{}", &k[..4], &k[k.len()-4..]) } else { "***".into() }).unwrap_or_else(|| "(env var)".into())),
                ("API Model", cfg_draft.api_model.clone().unwrap_or_else(|| "(default)".into())),
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
        super::app::ModalState::CodeBlockYanker { blocks, selected_index, preview_scroll } => {
            let block = Block::default()
                .title(Span::styled(" 📋 Code Block Yanker ", Style::default().fg(TEXT).add_modifier(Modifier::BOLD)))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(BLUE));

            let inner_area = block.inner(popup_area);
            f.render_widget(block, popup_area);

            let vertical_layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(1)])
                .split(inner_area);

            let horizontal_layout = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
                .split(vertical_layout[0]);

            // Render list on the left side
            let mut items = Vec::new();
            for (i, (lang, code)) in blocks.iter().enumerate() {
                let is_active = *selected_index == i;
                let prefix = if is_active { " > " } else { "   " };
                let style = if is_active {
                    Style::default().fg(GREEN).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(TEXT)
                };

                // Single line preview
                let first_line = code.lines().next().unwrap_or("").trim();
                let preview = if first_line.len() > 18 {
                    format!("{}...", &first_line[..18])
                } else {
                    first_line.to_string()
                };

                let text = format!("{}[{}] {}: {}", prefix, i + 1, lang, preview);
                items.push(ListItem::new(text).style(style));
            }

            let list_block = Block::default()
                .title(" Blocks ")
                .borders(Borders::RIGHT)
                .border_style(Style::default().fg(SURFACE1));
            
            let list_inner = list_block.inner(horizontal_layout[0]);
            f.render_widget(list_block, horizontal_layout[0]);
            f.render_widget(List::new(items), list_inner);

            // Render preview on the right side
            if let Some((lang, code)) = blocks.get(*selected_index) {
                let preview_block = Block::default()
                    .title(format!(" Preview ({}) ", lang))
                    .borders(Borders::NONE);
                
                let lines: Vec<Line> = code.lines().map(|line| Line::from(line)).collect();
                let total_lines = lines.len();
                let scroll_pos = (*preview_scroll).min(total_lines.saturating_sub(horizontal_layout[1].height as usize));
                
                let paragraph = Paragraph::new(lines)
                    .block(preview_block)
                    .wrap(Wrap { trim: false })
                    .scroll((scroll_pos as u16, 0))
                    .style(Style::default().fg(TEXT));

                f.render_widget(paragraph, horizontal_layout[1]);

                // Scrollbar for the preview
                if total_lines > horizontal_layout[1].height as usize {
                    let mut scrollbar_state = ScrollbarState::default()
                        .content_length(total_lines)
                        .position(scroll_pos);
                    f.render_stateful_widget(
                        Scrollbar::default()
                            .orientation(ScrollbarOrientation::VerticalRight)
                            .begin_symbol(Some("↑"))
                            .end_symbol(Some("↓"))
                            .track_symbol(Some("│"))
                            .thumb_symbol("█")
                            .style(Style::default().fg(SURFACE1)),
                        horizontal_layout[1],
                        &mut scrollbar_state,
                    );
                }
            }

            // Render footer
            let footer = Line::from(vec![
                Span::styled(" Ctrl+Shift+C ", Style::default().fg(CRUST).bg(GREEN).add_modifier(Modifier::BOLD)),
                Span::raw(" Yank | "),
                Span::styled(" Up/Down ", Style::default().fg(CRUST).bg(BLUE).add_modifier(Modifier::BOLD)),
                Span::raw(" Select block | "),
                Span::styled(" PgUp/PgDn ", Style::default().fg(CRUST).bg(MAUVE).add_modifier(Modifier::BOLD)),
                Span::raw(" Scroll preview | "),
                Span::styled(" Esc ", Style::default().fg(CRUST).bg(RED).add_modifier(Modifier::BOLD)),
                Span::raw(" Close"),
            ]);
            f.render_widget(Paragraph::new(footer).alignment(ratatui::layout::Alignment::Center).bg(SURFACE0), vertical_layout[1]);
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