//! Rendering. Turns `&mut App` into a frame.
//!
//! Takes `&mut App` rather than `&App` only so the renderer can report the
//! real list height back into `app.page_size` — paging keys should move by a
//! screenful of whatever the terminal currently is.

use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap,
};
use ratatui::Frame;

use crate::app::{App, Mode, Priority, Tab};

/// Below this width the detail pane is dropped and the list goes full-width.
pub const NARROW_WIDTH: u16 = 60;

pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    let filter_height = if matches!(app.mode, Mode::Filter { .. }) {
        1
    } else {
        0
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),             // tab bar
            Constraint::Min(3),                // content
            Constraint::Length(filter_height), // filter input, when open
            Constraint::Length(1),             // status bar
        ])
        .split(area);

    render_tabs(frame, app, chunks[0]);
    match app.tab {
        Tab::Tasks => render_tasks(frame, app, chunks[1]),
        Tab::Stats => render_stats(frame, app, chunks[1]),
        Tab::Logs => render_logs(frame, app, chunks[1]),
    }
    if filter_height == 1 {
        render_filter_input(frame, app, chunks[2]);
    }
    render_status(frame, app, chunks[3]);

    match app.mode {
        Mode::Help => render_help(frame, area),
        Mode::ConfirmDelete => render_confirm(frame, app, area),
        _ => {}
    }
}

fn render_tabs(frame: &mut Frame, app: &App, area: Rect) {
    let titles: Vec<Line> = Tab::ALL
        .iter()
        .map(|t| Line::from(Span::raw(t.title())))
        .collect();
    let selected = Tab::ALL.iter().position(|t| *t == app.tab).unwrap();
    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL).title(" taskboard "))
        .select(selected)
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_widget(tabs, area);
}

fn render_tasks(frame: &mut Frame, app: &mut App, area: Rect) {
    // Responsive: the detail pane is the first thing to go when it's tight.
    let (list_area, detail_area) = if area.width >= NARROW_WIDTH {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
            .split(area);
        (cols[0], Some(cols[1]))
    } else {
        (area, None)
    };

    app.page_size = usize::from(list_area.height.saturating_sub(2)).max(1);

    let visible = app.visible();
    let items: Vec<ListItem> = visible
        .iter()
        .map(|&i| {
            let task = &app.tasks[i];
            let (mark, mark_style) = if task.done {
                ("[x] ", Style::default().fg(Color::Green))
            } else {
                ("[ ] ", Style::default().fg(Color::DarkGray))
            };
            let priority_style = match task.priority {
                Priority::High => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                Priority::Medium => Style::default().fg(Color::Yellow),
                Priority::Low => Style::default().fg(Color::Green),
            };
            let title_style = if task.done {
                Style::default().add_modifier(Modifier::DIM)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::styled(mark, mark_style),
                Span::styled(format!("{:<4} ", task.priority.label()), priority_style),
                Span::styled(task.title.clone(), title_style),
            ]))
        })
        .collect();

    let title = if app.filter.is_empty() {
        format!(" tasks ({}) ", visible.len())
    } else {
        format!(" tasks ({}) filtered ", visible.len())
    };

    if items.is_empty() {
        let empty = Paragraph::new("no tasks match")
            .block(Block::default().borders(Borders::ALL).title(title))
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(empty, list_area);
    } else {
        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(title))
            .highlight_style(
                Style::default()
                    .add_modifier(Modifier::REVERSED)
                    .add_modifier(Modifier::BOLD),
            );
        let mut state = ListState::default();
        state.select(Some(app.selected.min(visible.len().saturating_sub(1))));
        frame.render_stateful_widget(list, list_area, &mut state);
    }

    if let Some(detail_area) = detail_area {
        render_detail(frame, app, detail_area);
    }
}

fn render_detail(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default().borders(Borders::ALL).title(" detail ");
    let Some(task) = app.selected_task() else {
        frame.render_widget(
            Paragraph::new("nothing selected")
                .block(block)
                .style(Style::default().fg(Color::DarkGray)),
            area,
        );
        return;
    };

    let status = if task.done { "done" } else { "open" };
    let status_color = if task.done { Color::Green } else { Color::Yellow };

    let body = vec![
        Line::from(vec![
            Span::styled("title    ", Style::default().fg(Color::Cyan)),
            Span::styled(task.title.clone(), Style::default().add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("status   ", Style::default().fg(Color::Cyan)),
            Span::styled(status, Style::default().fg(status_color)),
        ]),
        Line::from(vec![
            Span::styled("priority ", Style::default().fg(Color::Cyan)),
            Span::raw(task.priority.label()),
        ]),
        Line::from(vec![
            Span::styled("tags     ", Style::default().fg(Color::Cyan)),
            Span::raw(task.tags.join(", ")),
        ]),
        Line::from(""),
        Line::from(Span::raw(task.notes.clone())),
    ];

    frame.render_widget(
        Paragraph::new(body).block(block).wrap(Wrap { trim: true }),
        area,
    );
}

fn render_stats(frame: &mut Frame, app: &App, area: Rect) {
    let total = app.tasks.len();
    let done = app.done_count();
    let bar = |n: usize| "█".repeat(n);

    let lines = vec![
        Line::from(vec![
            Span::styled("total    ", Style::default().fg(Color::Cyan)),
            Span::raw(format!("{total}")),
        ]),
        Line::from(vec![
            Span::styled("done     ", Style::default().fg(Color::Cyan)),
            Span::styled(format!("{done}"), Style::default().fg(Color::Green)),
            Span::raw(format!("/{total}")),
        ]),
        Line::from(""),
        stat_row("HIGH", app.count_by(Priority::High), Color::Red, &bar),
        stat_row("med ", app.count_by(Priority::Medium), Color::Yellow, &bar),
        stat_row("low ", app.count_by(Priority::Low), Color::Green, &bar),
    ];

    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title(" stats ")),
        area,
    );
}

fn stat_row<'a>(
    label: &'a str,
    count: usize,
    color: Color,
    bar: &dyn Fn(usize) -> String,
) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!("{label}     "), Style::default().fg(color)),
        Span::styled(bar(count), Style::default().fg(color)),
        Span::raw(format!(" {count}")),
    ])
}

fn render_logs(frame: &mut Frame, app: &mut App, area: Rect) {
    app.page_size = usize::from(area.height.saturating_sub(2)).max(1);
    let items: Vec<ListItem> = app
        .logs
        .iter()
        .map(|l| ListItem::new(Line::from(Span::raw(l.clone()))))
        .collect();
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" logs ({}) ", app.logs.len())),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut state = ListState::default();
    state.select(Some(app.log_offset.min(app.logs.len().saturating_sub(1))));
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_filter_input(frame: &mut Frame, app: &App, area: Rect) {
    let Mode::Filter { draft } = &app.mode else {
        return;
    };
    let prompt = "/";
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(prompt, Style::default().fg(Color::Yellow)),
            Span::raw(draft.clone()),
        ])),
        area,
    );
    // A visible, correctly placed cursor is part of the UI contract for a
    // text input — and it's observable, so tests can assert on it.
    let x = area.x + prompt.len() as u16 + draft.chars().count() as u16;
    frame.set_cursor_position(Position::new(x.min(area.right().saturating_sub(1)), area.y));
}

fn render_status(frame: &mut Frame, app: &App, area: Rect) {
    let mode = if app.shutting_down {
        "SAVING"
    } else {
        match app.mode {
            Mode::Normal => "NORMAL",
            Mode::Filter { .. } => "FILTER",
            Mode::ConfirmDelete => "CONFIRM",
            Mode::Help => "HELP",
        }
    };

    let position = match app.tab {
        Tab::Tasks => {
            let len = app.visible().len();
            let shown = if len == 0 { 0 } else { app.selected + 1 };
            format!("{shown}/{len}")
        }
        Tab::Logs => format!("{}/{}", app.log_offset + 1, app.logs.len()),
        Tab::Stats => "-".to_string(),
    };

    let mut spans = vec![
        Span::styled(
            format!(" {mode} "),
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(" {} ", app.tab.title())),
        Span::raw(format!("{position} ")),
    ];
    if !app.filter.is_empty() {
        spans.push(Span::styled(
            format!("filter:{} ", app.filter),
            Style::default().fg(Color::Yellow),
        ));
    }
    spans.push(Span::styled(
        "? help  q quit",
        Style::default().fg(Color::DarkGray),
    ));

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_help(frame: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(Span::styled(
            "keys",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("  j/k, up/down   move cursor"),
        Line::from("  PgUp/PgDn      move a page"),
        Line::from("  Home/End       first / last"),
        Line::from("  Tab/Shift-Tab  switch tab"),
        Line::from("  space          toggle done"),
        Line::from("  /              filter tasks"),
        Line::from("  d              delete task"),
        Line::from("  ? or F1        this help"),
        Line::from("  q              quit"),
        Line::from("  Ctrl-C         interrupt (exit 130)"),
    ];
    let popup = popup_area(area, 40, lines.len() as u16 + 2);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" help ")
                .style(Style::default().fg(Color::White)),
        ),
        popup,
    );
}

fn render_confirm(frame: &mut Frame, app: &App, area: Rect) {
    let title = app
        .selected_task()
        .map(|t| t.title.clone())
        .unwrap_or_default();
    let lines = vec![
        Line::from("delete this task?"),
        Line::from(Span::styled(
            title,
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("[y]", Style::default().fg(Color::Red)),
            Span::raw(" delete   "),
            Span::styled("[n]", Style::default().fg(Color::Green)),
            Span::raw(" cancel"),
        ]),
    ];
    let popup = popup_area(area, 44, 6);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" confirm ")
                    .style(Style::default().fg(Color::Red)),
            )
            .wrap(Wrap { trim: true }),
        popup,
    );
}

/// Center a fixed-size popup. Fixed rather than percentage-based so the
/// popup's contents don't reflow with the terminal size — one less thing to
/// make snapshots size-dependent.
fn popup_area(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    }
}
