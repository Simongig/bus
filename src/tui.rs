//! Terminal UI: the day's route, stop details, the edit form and notifications.

use std::time::Duration;

use anyhow::Result;
use chrono::{Local, NaiveDate, NaiveDateTime, Timelike};
use ratatui::{
    crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, Wrap},
    DefaultTerminal, Frame,
};

use crate::model::{self, fmt_duration, fmt_time, Day, DodItem, Stop, DAY_END, GRID};
use crate::notify;
use crate::schedule::{self, Alert};
use crate::store::Store;

/// The timeline always shows at least this window.
const VIEW_START: i32 = 6 * 60;
const VIEW_END: i32 = 23 * 60;
const BREAK_MINUTES: i32 = 15;
const LUNCH_MINUTES: i32 = 60;
/// Start for new stops on an empty day that isn't today.
const DEFAULT_START: i32 = 9 * 60;
const DEFAULT_LUNCH_START: i32 = 12 * 60;
const DEFAULT_DURATION: i32 = 60;

pub fn run(store: Box<dyn Store>) -> Result<()> {
    let mut app = App::new(store)?;
    let mut terminal = ratatui::init();
    let result = app.run(&mut terminal);
    ratatui::restore();
    result
}

fn now() -> NaiveDateTime {
    Local::now().naive_local()
}

fn minute_of(t: NaiveDateTime) -> i32 {
    (t.hour() * 60 + t.minute()) as i32
}

enum Mode {
    Normal,
    Help,
    ConfirmDelete,
    Form(Form),
}

struct App {
    store: Box<dyn Store>,
    day: Day,
    selected: usize,
    /// First visible timeline row.
    scroll: usize,
    /// Scroll so the selection sits near the top third on the next draw.
    recenter: bool,
    mode: Mode,
    message: Option<String>,
    last_tick: NaiveDateTime,
    notify_failed: bool,
    quit: bool,
}

impl App {
    fn new(store: Box<dyn Store>) -> Result<Self> {
        let now = now();
        let day = store.load_day(now.date())?;
        let mut app = Self {
            store,
            day,
            selected: 0,
            scroll: 0,
            recenter: true,
            mode: Mode::Normal,
            message: None,
            last_tick: now,
            notify_failed: false,
            quit: false,
        };
        app.select_now();
        Ok(app)
    }

    fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while !self.quit {
            terminal.draw(|f| self.draw(f))?;
            if event::poll(Duration::from_millis(500))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        self.on_key(key);
                    }
                }
            }
            self.tick();
        }
        Ok(())
    }

    fn is_today(&self) -> bool {
        self.day.date == now().date()
    }

    /// Selects the running stop, else the next one, else the last one.
    fn select_now(&mut self) {
        self.selected = 0;
        if self.is_today() {
            let minute = minute_of(now());
            self.selected = self
                .day
                .stops
                .iter()
                .position(|s| s.end() > minute)
                .unwrap_or(self.day.stops.len().saturating_sub(1));
        }
        self.recenter = true;
    }

    fn selected_index(&self) -> Option<usize> {
        (self.selected < self.day.stops.len()).then_some(self.selected)
    }

    /// Applies `change` to a copy of the day and saves it. On success the
    /// returned index becomes the selection. Returns whether it worked.
    fn mutate(&mut self, change: impl FnOnce(&mut Day) -> Result<usize, String>) -> bool {
        let mut next = self.day.clone();
        match change(&mut next) {
            Ok(selected) => match self.store.save_day(&next) {
                Ok(()) => {
                    self.day = next;
                    self.selected = selected;
                    true
                }
                Err(e) => {
                    self.message = Some(format!("Save failed: {e}"));
                    false
                }
            },
            Err(msg) => {
                self.message = Some(msg);
                false
            }
        }
    }

    fn go_to(&mut self, date: Option<NaiveDate>) {
        let Some(date) = date else { return };
        match self.store.load_day(date) {
            Ok(day) => {
                self.day = day;
                self.select_now();
            }
            Err(e) => self.message = Some(format!("Load failed: {e}")),
        }
    }

    /// Where a new stop goes: after the selected stop, else now (today), else `fallback`.
    fn default_start(&self, fallback: i32) -> i32 {
        if let Some(stop) = self.selected_index().map(|i| &self.day.stops[i]) {
            stop.end()
        } else if self.is_today() {
            model::ceil_to_grid(minute_of(now()))
        } else {
            fallback
        }
    }

    fn insert_quick(&mut self, title: &str, duration: i32, fallback: i32) {
        let stop = Stop::new(title, self.default_start(fallback), duration);
        self.mutate(|day| day.insert(stop));
    }

    fn toggle_dod(&mut self, stop: usize, item: Option<usize>) {
        self.mutate(|day| {
            let dod = &mut day.stops[stop].dod;
            let index = match item {
                Some(i) if i < dod.len() => i,
                Some(_) => return Err("No such item".into()),
                None => dod
                    .iter()
                    .position(|d| !d.done)
                    .ok_or("Nothing left to check ✓")?,
            };
            dod[index].done = !dod[index].done;
            Ok(stop)
        });
    }

    // ── Time ────────────────────────────────────────────────────────────

    /// Sends notifications for minutes passed since the last tick and rolls
    /// over to the new day at midnight.
    fn tick(&mut self) {
        let now = now();
        let last = self.last_tick;
        if now.date() == last.date() && minute_of(now) == minute_of(last) {
            return;
        }
        self.last_tick = now;

        let mut alerts = Vec::new();
        if now.date() == last.date() {
            alerts.extend(self.alerts_for(now.date(), minute_of(last), minute_of(now)));
        } else {
            alerts.extend(self.alerts_for(last.date(), minute_of(last), DAY_END));
            alerts.extend(self.alerts_for(now.date(), -1, minute_of(now)));
            if self.day.date == last.date() {
                self.go_to(Some(now.date()));
            }
        }
        for alert in alerts {
            if let Err(e) = notify::send(&alert.summary, &alert.body) {
                if !self.notify_failed {
                    self.notify_failed = true;
                    self.message = Some(format!("Notifications unavailable: {e}"));
                }
            }
        }
    }

    fn alerts_for(&self, date: NaiveDate, from: i32, to: i32) -> Vec<Alert> {
        if self.day.date == date {
            return schedule::alerts(&self.day, from, to);
        }
        self.store
            .load_day(date)
            .map(|day| schedule::alerts(&day, from, to))
            .unwrap_or_default()
    }

    // ── Keys ────────────────────────────────────────────────────────────

    fn on_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        match std::mem::replace(&mut self.mode, Mode::Normal) {
            Mode::Normal => self.on_normal_key(key),
            Mode::Help => {}
            Mode::ConfirmDelete => {
                if let (KeyCode::Char('y'), Some(i)) = (key.code, self.selected_index()) {
                    self.mutate(|day| {
                        day.remove(i);
                        Ok(i.min(day.stops.len().saturating_sub(1)))
                    });
                }
            }
            Mode::Form(form) => {
                if let Some(form) = self.on_form_key(form, key) {
                    self.mode = Mode::Form(form);
                }
            }
        }
    }

    fn on_normal_key(&mut self, key: KeyEvent) {
        self.message = None;
        let sel = self.selected_index();
        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => self.mode = Mode::Help,
            KeyCode::Char('j') | KeyCode::Down => {
                if self.selected + 1 < self.day.stops.len() {
                    self.selected += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Char('J') => {
                if let Some(i) = sel {
                    self.mutate(|day| day.move_by(i, GRID));
                }
            }
            KeyCode::Char('K') => {
                if let Some(i) = sel {
                    self.mutate(|day| day.move_by(i, -GRID));
                }
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                if let Some(i) = sel {
                    self.mutate(|day| day.resize_by(i, GRID));
                }
            }
            KeyCode::Char('-') => {
                if let Some(i) = sel {
                    self.mutate(|day| day.resize_by(i, -GRID));
                }
            }
            KeyCode::Char('a') => {
                self.mode = Mode::Form(Form::new_stop(
                    self.default_start(DEFAULT_START),
                    DEFAULT_DURATION,
                ));
            }
            KeyCode::Char('e') | KeyCode::Enter => {
                if let Some(i) = sel {
                    self.mode = Mode::Form(Form::edit(i, &self.day.stops[i]));
                }
            }
            KeyCode::Char('d') => {
                if sel.is_some() {
                    self.mode = Mode::ConfirmDelete;
                }
            }
            KeyCode::Char('b') => self.insert_quick("Break", BREAK_MINUTES, DEFAULT_START),
            KeyCode::Char('B') => self.insert_quick("Lunch", LUNCH_MINUTES, DEFAULT_LUNCH_START),
            KeyCode::Char('h') | KeyCode::Left => self.go_to(self.day.date.pred_opt()),
            KeyCode::Char('l') | KeyCode::Right => self.go_to(self.day.date.succ_opt()),
            KeyCode::Char('t') => self.go_to(Some(now().date())),
            KeyCode::Char(' ') => {
                if let Some(i) = sel {
                    self.toggle_dod(i, None);
                }
            }
            KeyCode::Char(c @ '1'..='9') => {
                if let Some(i) = sel {
                    self.toggle_dod(i, Some(c as usize - '1' as usize));
                }
            }
            _ => {}
        }
    }

    /// Returns the form if it should stay open.
    fn on_form_key(&mut self, mut form: Form, key: KeyEvent) -> Option<Form> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => return None,
            KeyCode::Char('s') if ctrl => return self.submit(form),
            KeyCode::Tab => form.next_field(),
            KeyCode::BackTab => form.prev_field(),
            KeyCode::Down => form.down(),
            KeyCode::Up => form.up(),
            KeyCode::Enter if form.field == Field::Dod => {
                if form.dod[form.dod_line].trim().is_empty() {
                    return self.submit(form);
                }
                form.dod_line += 1;
                form.dod.insert(form.dod_line, String::new());
            }
            KeyCode::Enter => form.next_field(),
            KeyCode::Backspace => form.backspace(),
            KeyCode::Char(c) if !ctrl => form.text_mut().push(c),
            _ => {}
        }
        Some(form)
    }

    fn submit(&mut self, form: Form) -> Option<Form> {
        let stop = match form.to_stop() {
            Ok(stop) => stop,
            Err(e) => {
                self.message = Some(e);
                return Some(form);
            }
        };
        let editing = form.editing;
        let saved = self.mutate(|day| match editing {
            Some(i) => day.replace(i, stop),
            None => day.insert(stop),
        });
        if saved {
            self.message = None;
            None
        } else {
            Some(form)
        }
    }

    // ── Drawing ─────────────────────────────────────────────────────────

    fn draw(&mut self, f: &mut Frame) {
        let [header, body, footer] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .areas(f.area());
        let [route, details] =
            Layout::horizontal([Constraint::Min(30), Constraint::Length(42)]).areas(body);
        self.draw_header(f, header);
        self.draw_route(f, route);
        self.draw_details(f, details);
        self.draw_footer(f, footer);
        match &self.mode {
            Mode::Help => draw_help(f),
            Mode::Form(form) => form.draw(f),
            Mode::Normal | Mode::ConfirmDelete => {}
        }
    }

    fn draw_header(&self, f: &mut Frame, area: Rect) {
        let today = now().date();
        let tag = match self.day.date {
            d if d == today => " (today)",
            d if Some(d) == today.succ_opt() => " (tomorrow)",
            d if Some(d) == today.pred_opt() => " (yesterday)",
            _ => "",
        };
        let line = Line::from(vec![
            Span::styled(
                " bus ",
                Style::new()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {}{tag}", self.day.date.format("%a %d %b %Y")),
                Style::new().add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    "   {} stops · {} planned",
                    self.day.stops.len(),
                    fmt_duration(self.day.planned_minutes())
                ),
                Style::new().fg(Color::Gray),
            ),
        ]);
        f.render_widget(Paragraph::new(line), area);
    }

    /// Visible time range: 06:00–23:00, widened to whole hours to fit all stops and now.
    fn view_bounds(&self, now_min: Option<i32>) -> (i32, i32) {
        let mut first = VIEW_START;
        let mut last = VIEW_END;
        for t in self
            .day
            .stops
            .iter()
            .flat_map(|s| [s.start, s.end()])
            .chain(now_min)
        {
            first = first.min(t);
            last = last.max(t + 1);
        }
        (first / 60 * 60, ((last + 59) / 60 * 60).min(DAY_END))
    }

    fn draw_route(&mut self, f: &mut Frame, area: Rect) {
        let block = Block::bordered().title(" Route ");
        let inner = block.inner(area);
        f.render_widget(block, area);

        let now_min = self.is_today().then(|| minute_of(now()));
        let (start, end) = self.view_bounds(now_min);
        let rows = ((end - start) / GRID) as usize;
        let height = inner.height as usize;
        let row_of = |m: i32| ((m - start) / GRID) as usize;

        let target = match self.selected_index() {
            Some(i) => Some((
                row_of(self.day.stops[i].start),
                row_of(self.day.stops[i].end() - 1),
            )),
            None => now_min.map(|m| (row_of(m), row_of(m))),
        };
        if let Some((first, last)) = target {
            if self.recenter {
                self.scroll = first.saturating_sub(height / 3);
                self.recenter = false;
            }
            if last >= self.scroll + height {
                self.scroll = (last + 1).saturating_sub(height);
            }
            if first < self.scroll {
                self.scroll = first;
            }
        }
        self.scroll = self.scroll.min(rows.saturating_sub(height));

        let content_width = (inner.width as usize).saturating_sub(7);
        let mut lines = Vec::with_capacity(height);
        for row in self.scroll..(self.scroll + height).min(rows) {
            let t = start + row as i32 * GRID;
            let label = if t % 60 == 0 {
                fmt_time(t)
            } else {
                " ".repeat(5)
            };
            let marker = if now_min.is_some_and(|m| t <= m && m < t + GRID) {
                Span::styled(
                    "▶",
                    Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
                )
            } else {
                Span::raw(" ")
            };
            let (text, style) = match self.day.stops.iter().position(|s| s.contains(t)) {
                Some(i) => {
                    let stop = &self.day.stops[i];
                    let text = if t == stop.start {
                        format!(
                            " {} · {}{}",
                            stop.title,
                            fmt_duration(stop.duration),
                            dod_progress(stop)
                        )
                    } else {
                        String::new()
                    };
                    (text, self.stop_style(i, now_min))
                }
                None => {
                    let gap_begins = t == start || self.day.stops.iter().any(|s| s.end() == t);
                    let text = if gap_begins {
                        let until = self
                            .day
                            .stops
                            .iter()
                            .find(|s| s.start > t)
                            .map_or(end, |s| s.start);
                        format!(" free {}", fmt_duration(until - t))
                    } else {
                        String::new()
                    };
                    (text, Style::new().fg(Color::DarkGray))
                }
            };
            lines.push(Line::from(vec![
                Span::styled(label, Style::new().fg(Color::Gray)),
                Span::raw(" "),
                marker,
                Span::styled(pad(&text, content_width), style),
            ]));
        }
        f.render_widget(Paragraph::new(lines), inner);
    }

    fn stop_style(&self, index: usize, now_min: Option<i32>) -> Style {
        let stop = &self.day.stops[index];
        let bg = if index == self.selected {
            Color::Blue
        } else if now_min.is_some_and(|m| stop.contains(m)) {
            Color::Green
        } else if index.is_multiple_of(2) {
            Color::Indexed(238)
        } else {
            Color::Indexed(240)
        };
        let fg = if bg == Color::Green {
            Color::Black
        } else {
            Color::White
        };
        Style::new().fg(fg).bg(bg).add_modifier(Modifier::BOLD)
    }

    fn draw_details(&self, f: &mut Frame, area: Rect) {
        let block = Block::bordered().title(" Stop ");
        let Some(stop) = self.selected_index().map(|i| &self.day.stops[i]) else {
            let text = "No stops yet.\n\na  add a stop\nb  add a break\nB  add lunch\n?  all keys";
            f.render_widget(Paragraph::new(text).block(block), area);
            return;
        };
        let mut lines = vec![
            Line::styled(
                stop.title.clone(),
                Style::new().add_modifier(Modifier::BOLD),
            ),
            Line::styled(
                format!(
                    "{}–{} · {}",
                    fmt_time(stop.start),
                    fmt_time(stop.end()),
                    fmt_duration(stop.duration)
                ),
                Style::new().fg(Color::Gray),
            ),
            Line::default(),
            Line::styled(
                "Definition of done",
                Style::new().add_modifier(Modifier::UNDERLINED),
            ),
        ];
        if stop.dod.is_empty() {
            lines.push(Line::styled(
                "(none; e to edit)",
                Style::new().fg(Color::DarkGray),
            ));
        }
        for (n, item) in stop.dod.iter().enumerate() {
            let (check, style) = if item.done {
                (
                    "[x]",
                    Style::new()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::CROSSED_OUT),
                )
            } else {
                ("[ ]", Style::new())
            };
            lines.push(Line::from(vec![
                Span::styled(format!("{} {check} ", n + 1), Style::new().fg(Color::Gray)),
                Span::styled(item.text.clone(), style),
            ]));
        }
        f.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .block(block),
            area,
        );
    }

    fn draw_footer(&self, f: &mut Frame, area: Rect) {
        let line = if let (Mode::ConfirmDelete, Some(i)) = (&self.mode, self.selected_index()) {
            Line::styled(
                format!(" Delete \"{}\"? y/n", self.day.stops[i].title),
                Style::new().fg(Color::Yellow),
            )
        } else if let Some(msg) = &self.message {
            Line::styled(format!(" {msg}"), Style::new().fg(Color::Yellow))
        } else {
            Line::styled(
                " a add · e edit · b break · B lunch · J/K move · +/- length · h/l day · ? help · q quit",
                Style::new().fg(Color::DarkGray),
            )
        };
        f.render_widget(Paragraph::new(line), area);
    }
}

fn dod_progress(stop: &Stop) -> String {
    if stop.dod.is_empty() {
        return String::new();
    }
    let done = stop.dod.iter().filter(|d| d.done).count();
    format!("  [{done}/{}]", stop.dod.len())
}

/// Truncates or pads with spaces to exactly `width` characters.
fn pad(text: &str, width: usize) -> String {
    let mut s: String = text.chars().take(width).collect();
    let len = s.chars().count();
    s.extend(std::iter::repeat_n(' ', width - len));
    s
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}

fn draw_help(f: &mut Frame) {
    const KEYS: &[(&str, &str)] = &[
        ("j / k", "select next / previous stop"),
        ("J / K", "move stop 15m later / earlier"),
        ("+ / -", "make stop 15m longer / shorter"),
        ("a", "add a stop"),
        ("e, Enter", "edit selected stop"),
        ("d", "delete selected stop"),
        ("b", "insert 15m break after selection"),
        ("B", "insert 60m lunch after selection"),
        ("Space", "check next open DoD item"),
        ("1-9", "toggle DoD item by number"),
        ("h / l", "previous / next day"),
        ("t", "jump to today"),
        ("q", "quit"),
    ];
    let mut lines: Vec<Line> = KEYS
        .iter()
        .map(|(key, what)| {
            Line::from(vec![
                Span::styled(format!(" {key:<10}"), Style::new().fg(Color::Yellow)),
                Span::raw(*what),
            ])
        })
        .collect();
    lines.push(Line::default());
    lines.push(Line::styled(
        " Moving or growing a stop pushes the ones it overlaps.",
        Style::new().fg(Color::Gray),
    ));
    lines.push(Line::styled(
        " Free time absorbs the push.",
        Style::new().fg(Color::Gray),
    ));
    lines.push(Line::default());
    lines.push(Line::styled(
        " Press any key to close",
        Style::new().fg(Color::DarkGray),
    ));

    let area = centered(f.area(), 60, lines.len() as u16 + 2);
    f.render_widget(Clear, area);
    f.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(" Keys ")),
        area,
    );
}

// ── Form ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Title,
    Start,
    Duration,
    Dod,
}

const FIELDS: [Field; 4] = [Field::Title, Field::Start, Field::Duration, Field::Dod];

struct Form {
    /// Index of the stop being edited; `None` for a new stop.
    editing: Option<usize>,
    field: Field,
    title: String,
    start: String,
    duration: String,
    /// One DoD item per line; always at least one line.
    dod: Vec<String>,
    dod_line: usize,
    /// Used to keep the done state of items whose text didn't change.
    previous_dod: Vec<DodItem>,
}

impl Form {
    fn new_stop(start: i32, duration: i32) -> Self {
        Self {
            editing: None,
            field: Field::Title,
            title: String::new(),
            start: fmt_time(start.min(DAY_END - GRID)),
            duration: fmt_duration(duration),
            dod: vec![String::new()],
            dod_line: 0,
            previous_dod: Vec::new(),
        }
    }

    fn edit(index: usize, stop: &Stop) -> Self {
        let mut dod: Vec<String> = stop.dod.iter().map(|d| d.text.clone()).collect();
        if dod.is_empty() {
            dod.push(String::new());
        }
        Self {
            editing: Some(index),
            field: Field::Title,
            title: stop.title.clone(),
            start: fmt_time(stop.start),
            duration: fmt_duration(stop.duration),
            dod_line: dod.len() - 1,
            dod,
            previous_dod: stop.dod.clone(),
        }
    }

    fn field_index(&self) -> usize {
        FIELDS.iter().position(|f| *f == self.field).unwrap_or(0)
    }

    fn next_field(&mut self) {
        self.field = FIELDS[(self.field_index() + 1) % FIELDS.len()];
    }

    fn prev_field(&mut self) {
        self.field = FIELDS[(self.field_index() + FIELDS.len() - 1) % FIELDS.len()];
    }

    fn down(&mut self) {
        if self.field == Field::Dod && self.dod_line + 1 < self.dod.len() {
            self.dod_line += 1;
        } else if self.field != Field::Dod {
            self.next_field();
        }
    }

    fn up(&mut self) {
        if self.field == Field::Dod && self.dod_line > 0 {
            self.dod_line -= 1;
        } else if self.field != Field::Title {
            self.prev_field();
        }
    }

    fn text_mut(&mut self) -> &mut String {
        match self.field {
            Field::Title => &mut self.title,
            Field::Start => &mut self.start,
            Field::Duration => &mut self.duration,
            Field::Dod => &mut self.dod[self.dod_line],
        }
    }

    /// Deletes a character; on an empty DoD line, removes the line.
    fn backspace(&mut self) {
        if self.field == Field::Dod && self.dod[self.dod_line].is_empty() && self.dod.len() > 1 {
            self.dod.remove(self.dod_line);
            self.dod_line = self.dod_line.saturating_sub(1);
        } else {
            self.text_mut().pop();
        }
    }

    fn to_stop(&self) -> Result<Stop, String> {
        let start = model::parse_time(&self.start)?;
        let duration = model::parse_duration(&self.duration)?;
        let mut stop = Stop::new(self.title.trim(), start, duration);
        stop.dod = self
            .dod
            .iter()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .map(|text| DodItem {
                text: text.to_string(),
                done: self.previous_dod.iter().any(|d| d.done && d.text == text),
            })
            .collect();
        Ok(stop)
    }

    fn draw(&self, f: &mut Frame) {
        let cursor = Span::styled("█", Style::new().fg(Color::Yellow));
        let field_line = |field: Field, label: &str, value: &str| {
            let active = self.field == field;
            let mut spans = vec![
                Span::styled(
                    format!("{} {label:<9}", if active { ">" } else { " " }),
                    Style::new().fg(if active { Color::Yellow } else { Color::Gray }),
                ),
                Span::raw(value.to_string()),
            ];
            if active {
                spans.push(cursor.clone());
            }
            Line::from(spans)
        };

        let mut lines = vec![
            field_line(Field::Title, "Title", &self.title),
            field_line(Field::Start, "Start", &self.start),
            field_line(Field::Duration, "Duration", &self.duration),
            Line::default(),
            Line::styled(
                format!(
                    "{} Definition of done (one per line)",
                    if self.field == Field::Dod { ">" } else { " " }
                ),
                Style::new().fg(if self.field == Field::Dod {
                    Color::Yellow
                } else {
                    Color::Gray
                }),
            ),
        ];
        for (i, text) in self.dod.iter().enumerate() {
            let mut spans = vec![Span::raw(format!("    • {text}"))];
            if self.field == Field::Dod && i == self.dod_line {
                spans.push(cursor.clone());
            }
            lines.push(Line::from(spans));
        }
        lines.push(Line::default());
        let hint = Style::new().fg(Color::DarkGray);
        lines.push(Line::styled(
            "  Tab/↑↓ fields · Enter next field / new item",
            hint,
        ));
        lines.push(Line::styled(
            "  Enter on empty item or Ctrl+S saves · Esc cancels",
            hint,
        ));

        let title = if self.editing.is_some() {
            " Edit stop "
        } else {
            " New stop "
        };
        let area = centered(f.area(), 60, lines.len() as u16 + 2);
        f.render_widget(Clear, area);
        f.render_widget(
            Paragraph::new(lines).block(Block::bordered().title(title)),
            area,
        );
    }
}
