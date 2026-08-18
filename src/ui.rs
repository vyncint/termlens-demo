//! Rendering. Turns `&mut App` into a frame.
//!
//! Takes `&mut App` rather than `&App` so the renderer can report two
//! things back: the real list height (paging keys should move by a
//! screenful of whatever the terminal currently is) and where the
//! hyperlink label landed (`main` re-emits it wrapped in `OSC 8`, which no
//! cell-based buffer can express).

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph, Tabs, Wrap,
};

use crate::app::{App, Lane, Mode, Priority, Tab};

/// Below this width the detail pane is dropped and the list goes full-width.
pub const NARROW_WIDTH: u16 = 60;

pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    app.link_hint = None;

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
        Tab::Board => render_board(frame, app, chunks[1]),
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
        Mode::Running { pct } => render_running(frame, app, area, pct),
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

/// The row for one task in the list: status mark, priority, an overdue
/// badge, and the title.
///
/// Two attributes here are chosen because a harness may not model them:
/// a finished title is struck through (`SGR 9`), and an overdue badge
/// blinks (`SGR 5`). Both are visible to a person and, as
/// `tests/hard.rs` records, invisible to termlens.
fn task_line(app: &App, index: usize) -> Line<'static> {
    let task = &app.tasks[index];
    let (mark, mark_style) = if task.done {
        ("[x] ", Style::default().fg(Color::Green))
    } else if task.started {
        ("[~] ", Style::default().fg(Color::Cyan))
    } else {
        ("[ ] ", Style::default().fg(Color::DarkGray))
    };
    let priority_style = match task.priority {
        Priority::High => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        Priority::Medium => Style::default().fg(Color::Yellow),
        Priority::Low => Style::default().fg(Color::Green),
    };
    let title_style = if task.done {
        Style::default()
            .add_modifier(Modifier::DIM)
            .add_modifier(Modifier::CROSSED_OUT)
    } else {
        Style::default()
    };

    let mut spans = vec![
        Span::styled(mark, mark_style),
        Span::styled(format!("{:<4} ", task.priority.label()), priority_style),
    ];
    if task.overdue {
        spans.push(Span::styled(
            "! ",
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::SLOW_BLINK),
        ));
    }
    spans.push(Span::styled(task.title.clone(), title_style));
    Line::from(spans)
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
        .map(|&i| ListItem::new(task_line(app, i)))
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

fn render_detail(frame: &mut Frame, app: &mut App, area: Rect) {
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

    let status = if task.done {
        "done"
    } else if task.started {
        "doing"
    } else {
        "open"
    };
    let status_color = if task.done {
        Color::Green
    } else if task.started {
        Color::Cyan
    } else {
        Color::Yellow
    };

    let label = |text: &'static str| Span::styled(text, Style::default().fg(Color::Cyan));

    let mut body = vec![
        Line::from(vec![
            label("title    "),
            Span::styled(
                task.title.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            label("status   "),
            Span::styled(status, Style::default().fg(status_color)),
        ]),
        Line::from(vec![label("priority "), Span::raw(task.priority.label())]),
        Line::from(vec![label("tags     "), Span::raw(task.tags.join(", "))]),
    ];

    // A concealed field: `SGR 8` hides it in a real terminal, and the
    // characters sit in the grid regardless.
    if let Some(secret) = &task.secret {
        body.push(Line::from(vec![
            label("secret   "),
            Span::styled(
                secret.clone(),
                Style::default().add_modifier(Modifier::HIDDEN),
            ),
        ]));
    }

    // The hyperlink is drawn here as a plain label; `main` re-emits it
    // wrapped in OSC 8 once the frame is otherwise complete, because a
    // cell buffer has nowhere to put a URL.
    let link_row = body.len();
    if task.url.is_some() {
        body.push(Line::from(vec![label("link     "), Span::raw("open ref")]));
    }

    body.push(Line::from(""));
    body.push(Line::from(Span::raw(task.notes.clone())));

    let url = task.url.clone();
    frame.render_widget(
        Paragraph::new(body)
            .block(block.clone())
            .wrap(Wrap { trim: true }),
        area,
    );

    if let Some(url) = url {
        // Inside the block's border: +1 for the border, +9 for the label.
        let x = area.x + 1 + 9;
        let y = area.y + 1 + link_row as u16;
        if y < area.bottom().saturating_sub(1) {
            app.link_hint = Some((x, y, "open ref".to_string(), url));
        }
    }
}

/// Three lanes side by side. Titles are truncated to the lane width, which
/// is where a double-width glyph can be cut in half — the case that makes
/// column arithmetic worth testing.
fn render_board(frame: &mut Frame, app: &mut App, area: Rect) {
    let lanes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
        ])
        .split(area);

    app.page_size = usize::from(area.height.saturating_sub(2)).max(1);

    for (i, lane) in Lane::ALL.iter().enumerate() {
        let indices = app.lane_tasks(*lane);
        let focused = i == app.lane;
        let items: Vec<ListItem> = indices
            .iter()
            .map(|&idx| {
                let task = &app.tasks[idx];
                let style = if task.done {
                    Style::default()
                        .add_modifier(Modifier::DIM)
                        .add_modifier(Modifier::CROSSED_OUT)
                } else {
                    Style::default()
                };
                ListItem::new(Line::from(Span::styled(task.title.clone(), style)))
            })
            .collect();

        let border_style = if focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(format!(" {} ({}) ", lane.title(), indices.len()));

        let list = List::new(items)
            .block(block)
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
        let mut state = ListState::default();
        if focused && !indices.is_empty() {
            state.select(Some(app.selected.min(indices.len() - 1)));
        }
        frame.render_stateful_widget(list, lanes[i], &mut state);
    }
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
        Line::from(vec![
            Span::styled("lanes    ", Style::default().fg(Color::Cyan)),
            Span::raw(format!(
                "{} todo, {} doing, {} done",
                app.lane_tasks(Lane::Todo).len(),
                app.lane_tasks(Lane::Doing).len(),
                app.lane_tasks(Lane::Done).len()
            )),
        ]),
        Line::from(""),
        stat_row("HIGH", app.count_by(Priority::High), Color::Red, &bar),
        stat_row("med ", app.count_by(Priority::Medium), Color::Yellow, &bar),
        stat_row("low ", app.count_by(Priority::Low), Color::Green, &bar),
    ];

    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" stats ")),
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
            Mode::Running { .. } => "RUNNING",
        }
    };

    let position = match app.tab {
        Tab::Tasks => {
            let len = app.visible().len();
            let shown = if len == 0 { 0 } else { app.selected + 1 };
            format!("{shown}/{len}")
        }
        Tab::Board => {
            let len = app.lane_tasks(Lane::ALL[app.lane]).len();
            let shown = if len == 0 {
                0
            } else {
                app.selected.min(len - 1) + 1
            };
            format!("{} {shown}/{len}", Lane::ALL[app.lane].title())
        }
        Tab::Logs => format!("{}/{}", app.log_offset + 1, app.logs.len()),
        Tab::Stats => "-".to_string(),
    };

    // Unfocused windows dim their chrome — the terminal told us so via
    // mode 1004.
    let mode_style = if app.focused {
        Style::default()
            .bg(Color::Blue)
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .bg(Color::DarkGray)
            .fg(Color::Gray)
            .add_modifier(Modifier::DIM)
    };

    let mut spans = vec![
        Span::styled(format!(" {mode} "), mode_style),
        Span::raw(format!(" {} ", app.tab.title())),
        Span::raw(format!("{position} ")),
    ];
    if !app.filter.is_empty() {
        spans.push(Span::styled(
            format!("filter:{} ", app.filter),
            Style::default().fg(Color::Yellow),
        ));
    }
    if app.high_contrast {
        spans.push(Span::styled(
            "HC ",
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ));
    }
    // The toast lives for exactly one frame. Anything that wants to assert
    // on it has to be able to observe a frame the app immediately replaces.
    if let Some(toast) = &app.toast {
        spans.push(Span::styled(
            format!("· {toast} "),
            Style::default().fg(Color::Green),
        ));
    }
    spans.push(Span::styled(
        "? help  q quit",
        Style::default().fg(Color::DarkGray),
    ));

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_running(frame: &mut Frame, app: &App, area: Rect, pct: u8) {
    let title = app
        .selected_task()
        .map(|t| t.title.clone())
        .unwrap_or_default();
    let popup = popup_area(area, 46, 5);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" running {pct}% "))
        .style(Style::default().fg(Color::Cyan));
    let inner = Rect {
        x: popup.x + 1,
        y: popup.y + 1,
        width: popup.width.saturating_sub(2),
        height: popup.height.saturating_sub(2),
    };
    frame.render_widget(block, popup);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(inner);
    frame.render_widget(Paragraph::new(Line::from(Span::raw(title))), rows[0]);
    frame.render_widget(
        Gauge::default()
            .gauge_style(Style::default().fg(Color::Cyan))
            .percent(u16::from(pct)),
        rows[1],
    );
}

fn render_help(frame: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(Span::styled(
            "keys",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("  j/k, up/down   move cursor"),
        Line::from("  h/l            board lane"),
        Line::from("  PgUp/PgDn      move a page"),
        Line::from("  Home/End       first / last"),
        Line::from("  Tab/Shift-Tab  switch tab"),
        Line::from("  space          toggle done"),
        Line::from("  /              filter tasks"),
        Line::from("  d              delete task"),
        Line::from("  r              run task"),
        Line::from("  y              yank title"),
        Line::from("  T              high contrast"),
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
