//! The full-screen console as a state machine that a terminal or a test draws (M4 hosting spec
//! §3.2): the title bar, the status strip, the log anchored to the bottom, the command palette
//! while it's open, a hairline and the prompt row. It holds no terminal: [`crate::run_tui`] feeds
//! it the keys and the server's messages, and draws it. Everything it draws goes through
//! [`clean`].
use std::cell::Cell;
use std::collections::VecDeque;
use std::ops::Range;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::Frame;
use ratatui::buffer::CellWidth;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Padding, Paragraph, Wrap};

use crate::text::{clean, hidden, plain};
use crate::theme::{Colours, Palette};
use crate::wire::{CommandHelp, INPUT_MAX, Level, LogLine, ServerMsg, State, Status};

/// The rows the log keeps to scroll back through: the server's lines, and the commands run here
/// with their answers.
pub const SCROLLBACK: usize = 5_000;
/// The commands ↑ and ↓ walk back through.
pub const HISTORY: usize = 100;
/// The narrowest terminal the console fills, in columns; a smaller one gets a note.
pub const MIN_WIDTH: u16 = 60;
/// The shortest terminal the console fills, in rows.
pub const MIN_HEIGHT: u16 = 15;
/// The most commands the palette lists at once; past that it scrolls to the chosen one.
const PALETTE_ROWS: usize = 8;
/// Where a log line's message starts: a column of padding, `HH:MM:SS`, the level padded to five,
/// and a space after each.
const MESSAGE_AT: usize = 16;
/// Where the line being typed starts, after the prompt's ` > `, and so where the commands run and
/// their answers show in the log, and where the palette's list starts.
const LINE_AT: usize = 3;
/// The keys, at the prompt's right while there's room.
const HINT: &str = "Tab completes · ↑ ↓ choose · Enter runs";

/// Where the console runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// In the terminal that started the server: Ctrl-C stops it, as `stop` does.
    InProcess,
    /// Attached to a running server: Ctrl-C, `exit` or `quit` leaves, and the server runs on.
    Attached,
}

/// Whether `line` leaves an attached console: `exit` or `quit`, in any case.
pub(crate) fn leaves(line: &str) -> bool {
    line.eq_ignore_ascii_case("exit") || line.eq_ignore_ascii_case("quit")
}

/// What a key asks of the console's loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Send this command line to the server.
    Run(String),
    /// Ctrl-C, or `exit` or `quit` when attached: stop the server in-process, or leave it when
    /// attached.
    Interrupt,
}

/// A row of the scrollback.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Row {
    Log(LogLine),
    /// A command run here, as typed.
    Command(String),
    /// A line of a command's answer, and whether the command ran.
    Answer {
        ok: bool,
        text: String,
    },
    /// The console's own words, such as the lines it skipped.
    Note(String),
}

/// A row of the scrollback, with the lines it takes on screen at the width it was last measured
/// at, so a draw adds up the log's lines without wrapping every row again.
#[derive(Debug)]
struct Entry {
    row: Row,
    lines: Cell<Option<(usize, usize)>>,
}

impl Entry {
    fn new(row: Row) -> Entry {
        Entry {
            row,
            lines: Cell::new(None),
        }
    }

    /// The lines the row takes at `width` columns.
    fn lines(&self, width: usize) -> usize {
        match self.lines.get() {
            Some((at, lines)) if at == width => lines,
            _ => {
                let lines = row_height(&self.row, width);
                self.lines.set(Some((width, lines)));
                lines
            }
        }
    }
}

/// The console's state: what it shows, the line being typed, the palette and the history.
#[derive(Debug)]
pub struct View {
    server: String,
    commands: Vec<CommandHelp>,
    mode: Mode,
    status: Option<Status>,
    rows: VecDeque<Entry>,
    /// How far the log is scrolled back: the lines hidden below its bottom, on screen at the last
    /// draw's width. PgUp, PgDn and the hairline's note count the same lines.
    scroll: usize,
    input: String,
    /// The cursor, as a byte offset into `input` at the start of a character.
    cursor: usize,
    /// Whether the palette is open; it lists the commands that start with the first word.
    open: bool,
    /// The palette's chosen row: Tab completes it.
    selected: usize,
    /// Whether ↑ or ↓ moved the choice since the line last changed: then Enter runs the chosen
    /// command. Otherwise Enter runs the line as typed, so `s` never stops the server alone.
    chosen: bool,
    history: VecDeque<String>,
    /// While walking the history: where in it, and the line typed before the walk began.
    recall: Option<usize>,
    draft: String,
    /// The log's height at the last draw, for PgUp and PgDn's pages.
    page: Cell<usize>,
    /// The log's width at the last draw, which a row added while scrolled back is measured at.
    log_width: Cell<usize>,
    /// How far back the log could scroll at the last draw, in lines: until the oldest row's first
    /// line is at the top.
    furthest: Cell<usize>,
}

impl View {
    pub fn new(server: String, commands: Vec<CommandHelp>, mode: Mode) -> View {
        View {
            server,
            commands,
            mode,
            status: None,
            rows: VecDeque::new(),
            scroll: 0,
            input: String::new(),
            cursor: 0,
            open: false,
            selected: 0,
            chosen: false,
            history: VecDeque::new(),
            recall: None,
            draft: String::new(),
            page: Cell::new(1),
            log_width: Cell::new(0),
            furthest: Cell::new(0),
        }
    }

    /// The line being typed.
    pub fn input(&self) -> &str {
        &self.input
    }

    /// The commands the palette lists, by name; empty while it's closed.
    pub fn palette(&self) -> Vec<&str> {
        self.listed().iter().map(|c| c.name.as_str()).collect()
    }

    pub fn on_server(&mut self, msg: ServerMsg) {
        match msg {
            ServerMsg::Hello {
                server, commands, ..
            } => {
                self.server = server;
                self.commands = commands;
                self.selected = 0;
                self.chosen = false;
            }
            ServerMsg::Log { line } => self.push(Row::Log(line)),
            ServerMsg::Skipped { lines } => {
                self.push(Row::Note(format!("… {lines} lines skipped")))
            }
            ServerMsg::Status { status } => self.status = Some(status),
            ServerMsg::Answer { ok, lines, .. } => {
                for text in lines {
                    self.push(Row::Answer { ok, text });
                }
            }
            ServerMsg::Bye { reason } => self.push(Row::Note(reason)),
        }
    }

    /// A note of the console's own in the log.
    pub(crate) fn note(&mut self, text: impl Into<String>) {
        self.push(Row::Note(text.into()));
    }

    pub fn on_key(&mut self, key: KeyEvent) -> Option<Action> {
        // Windows reports releases too.
        if key.kind == KeyEventKind::Release {
            return None;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            // AltGr comes as Ctrl and Alt together, and types characters.
            KeyCode::Char(c) if ctrl && !alt => match c.to_ascii_lowercase() {
                'c' => return self.interrupt(),
                'u' => {
                    self.input.clear();
                    self.cursor = 0;
                    self.edited();
                }
                _ => {}
            },
            KeyCode::Char(_) if alt && !ctrl => {}
            KeyCode::Char(c) => self.type_char(c),
            KeyCode::Enter => return self.enter(),
            KeyCode::Tab => self.tab(),
            KeyCode::Esc => {
                self.open = false;
                self.chosen = false;
            }
            KeyCode::Up => self.up(),
            KeyCode::Down => self.down(),
            KeyCode::PageUp => self.scroll = (self.scroll + self.step()).min(self.furthest.get()),
            KeyCode::PageDown => {
                self.scroll = self
                    .scroll
                    .min(self.furthest.get())
                    .saturating_sub(self.step());
            }
            KeyCode::Left => self.cursor = before(&self.input, self.cursor),
            KeyCode::Right => self.cursor = after(&self.input, self.cursor),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.input.len(),
            KeyCode::Backspace if self.cursor > 0 => {
                let from = before(&self.input, self.cursor);
                self.input.replace_range(from..self.cursor, "");
                self.cursor = from;
                self.edited();
            }
            KeyCode::Delete if self.cursor < self.input.len() => {
                let to = after(&self.input, self.cursor);
                self.input.replace_range(self.cursor..to, "");
                self.edited();
            }
            _ => {}
        }
        None
    }

    pub fn draw(&self, frame: &mut Frame<'_>, palette: &Palette, now_ms: i64) {
        let area = frame.area();
        if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
            let note = Paragraph::new(
                "Make this window bigger: the console needs 60 columns and 15 rows.",
            )
            .wrap(Wrap { trim: true })
            .block(Block::new().padding(Padding::horizontal(1)))
            .style(Style::new().fg(palette.text_2).bg(palette.ink_950));
            frame.render_widget(note, area);
            return;
        }
        let listed = self.listed();
        let [title, status, log, list, hairline, prompt] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(listed.len().min(PALETTE_ROWS) as u16),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(area);
        self.draw_title(frame, title, palette);
        self.draw_status(frame, status, palette, now_ms);
        let scroll = self.draw_log(frame, log, palette);
        self.draw_palette(frame, list, palette, &listed);
        draw_hairline(frame, hairline, palette, scroll);
        self.draw_prompt(frame, prompt, palette);
    }

    /// Adds a row to the log. A log scrolled back stays on the lines it shows: the new row's
    /// lines join those hidden below.
    fn push(&mut self, row: Row) {
        let entry = Entry::new(row);
        if self.scroll > 0 {
            self.scroll = self
                .scroll
                .saturating_add(entry.lines(self.log_width.get()));
        }
        self.rows.push_back(entry);
        if self.rows.len() > SCROLLBACK {
            self.rows.pop_front();
        }
    }

    /// The commands the palette lists: those that start with the line's first word, in any case,
    /// as the server reads it. None while it's closed.
    fn listed(&self) -> Vec<&CommandHelp> {
        if !self.open {
            return Vec::new();
        }
        let word = first_word(&self.input);
        self.commands
            .iter()
            .filter(|c| {
                c.name
                    .get(..word.len())
                    .is_some_and(|start| start.eq_ignore_ascii_case(word))
            })
            .collect()
    }

    /// PgUp and PgDn's step: the log's height, less a row kept in sight.
    fn step(&self) -> usize {
        self.page.get().saturating_sub(1).max(1)
    }

    fn type_char(&mut self, c: char) {
        if hidden(c) || self.input.len() + c.len_utf8() > INPUT_MAX {
            return;
        }
        self.input.insert(self.cursor, c);
        self.cursor += c.len_utf8();
        self.edited();
    }

    /// After the line changed: the history walk ends, and the palette opens while the first word
    /// is being typed.
    fn edited(&mut self) {
        self.recall = None;
        self.open = typing_first_word(&self.input);
        self.selected = 0;
        self.chosen = false;
    }

    /// Tab opens the palette, or completes the chosen command when it lists some.
    fn tab(&mut self) {
        if self.listed().is_empty() {
            self.open = true;
            self.selected = 0;
            self.chosen = false;
        } else {
            self.complete();
        }
    }

    /// The chosen command's name in place of the line's first word.
    fn complete(&mut self) {
        let listed = self.listed();
        let Some(name) = listed
            .get(self.selected.min(listed.len().saturating_sub(1)))
            .map(|c| c.name.clone())
        else {
            return;
        };
        let start = self.input.len() - self.input.trim_start().len();
        let end = start + first_word(&self.input).len();
        if !plain(&name) || self.input.len() - (end - start) + name.len() > INPUT_MAX {
            return;
        }
        self.input.replace_range(start..end, &name);
        self.cursor = if self.cursor <= end {
            start + name.len()
        } else {
            self.cursor - end + start + name.len()
        };
        self.recall = None;
        self.chosen = false;
        self.selected = self
            .listed()
            .iter()
            .position(|c| c.name == name)
            .unwrap_or(0);
    }

    fn enter(&mut self) -> Option<Action> {
        if self.chosen {
            self.complete();
        }
        let line = self.input.trim().to_owned();
        self.input.clear();
        self.cursor = 0;
        self.open = false;
        self.selected = 0;
        self.chosen = false;
        self.recall = None;
        self.draft.clear();
        if line.is_empty() {
            return None;
        }
        if self.mode == Mode::Attached && leaves(&line) {
            return Some(Action::Interrupt);
        }
        if self.history.back().is_none_or(|last| *last != line) {
            self.history.push_back(line.clone());
            if self.history.len() > HISTORY {
                self.history.pop_front();
            }
        }
        self.scroll = 0;
        self.push(Row::Command(line.clone()));
        Some(Action::Run(line))
    }

    fn interrupt(&mut self) -> Option<Action> {
        if self.mode == Mode::InProcess {
            self.scroll = 0;
            self.push(Row::Command("stop".into()));
        }
        Some(Action::Interrupt)
    }

    /// ↑: the palette's row above, or an older command.
    fn up(&mut self) {
        if !self.listed().is_empty() {
            self.choose(self.selected.saturating_sub(1));
            return;
        }
        let at = match self.recall {
            None if self.history.is_empty() => return,
            None => {
                self.draft = std::mem::take(&mut self.input);
                self.history.len() - 1
            }
            Some(at) => at.saturating_sub(1),
        };
        self.recall = Some(at);
        self.input = self.history[at].clone();
        self.recalled();
    }

    /// ↓: the palette's row below, or a newer command, and past the newest the line typed before.
    fn down(&mut self) {
        let listed = self.listed().len();
        if listed > 0 {
            self.choose((self.selected + 1).min(listed - 1));
            return;
        }
        let Some(at) = self.recall else {
            return;
        };
        if at + 1 < self.history.len() {
            self.recall = Some(at + 1);
            self.input = self.history[at + 1].clone();
        } else {
            self.recall = None;
            self.input = std::mem::take(&mut self.draft);
        }
        self.recalled();
    }

    /// After ↑ or ↓ brought a line back: the cursor at its end, and the palette closed, so the
    /// next ↑ or ↓ walks on.
    fn recalled(&mut self) {
        self.cursor = self.input.len();
        self.open = false;
        self.chosen = false;
    }

    fn choose(&mut self, row: usize) {
        if row != self.selected {
            self.selected = row;
            self.chosen = true;
        }
    }

    fn draw_title(&self, frame: &mut Frame<'_>, area: Rect, p: &Palette) {
        let title = format!(" fenji-server — {}", clean(&self.server));
        frame.render_widget(
            Paragraph::new(title.as_str()).style(Style::new().fg(p.text_2).bg(p.ink_900)),
            area,
        );
        let keys = match self.mode {
            Mode::InProcess => "Ctrl-C stops the server",
            Mode::Attached => "attached · Ctrl-C or exit leaves",
        };
        // The log's times are UTC (Ruling HP6), said once, here: shorter where the server's name
        // leaves less room (the hosting plan's final review B, Minor 1).
        let taken = width(&title) + 2;
        let hints = [
            format!("{keys} · times in UTC"),
            format!("{keys} · UTC"),
            keys.to_owned(),
        ];
        let room = usize::from(area.width);
        if let Some(hint) = hints.iter().find(|h| taken + width(h) < room) {
            at_right(frame, area, hint, Style::new().fg(p.faint), taken);
        }
    }

    /// The status strip: the state word in its colour, the server's name in gold, then the facts
    /// that fit whole, in order. The host's maintenance message alone is shortened to fit.
    fn draw_status(&self, frame: &mut Frame<'_>, area: Rect, p: &Palette, now_ms: i64) {
        let gold = Style::new().fg(p.gold);
        let mut spans = vec![Span::raw(" ")];
        match &self.status {
            None => spans.push(Span::styled(clean(&self.server), gold)),
            Some(s) => {
                let (word, colour) = match s.state {
                    State::Online => ("online", p.ok),
                    State::Maintenance => ("maintenance", p.warn),
                    State::Stopping => ("stopping", p.danger),
                };
                let server = clean(&s.server);
                let version = clean(&s.version);
                let version = if version.starts_with('v') {
                    version.into_owned()
                } else {
                    format!("v{version}")
                };
                // Each fact, and whether it may be shortened to fit.
                let mut facts = vec![
                    (version, false),
                    (
                        format!("up {}", uptime(now_ms.saturating_sub(s.started_ms))),
                        false,
                    ),
                    (format!("{} of {} online", s.online, s.max), false),
                ];
                if let Some(message) = &s.maintenance {
                    facts.push((clean(message).into_owned(), true));
                }
                facts.push((format!("gate {}", s.gate_port), false));
                facts.push((format!("api {}", s.api_port), false));
                let separator = Span::styled(" · ", Style::new().fg(p.faint));
                let room = usize::from(area.width).saturating_sub(1);
                let mut used = 1 + word.len() + 3 + width(&server);
                spans.push(Span::styled(word, Style::new().fg(colour)));
                spans.push(separator.clone());
                spans.push(Span::styled(server, gold));
                for (fact, shortens) in facts {
                    let left = room.saturating_sub(used + 3);
                    let fact = if width(&fact) <= left {
                        fact
                    } else if shortens && left > 10 {
                        shortened(&fact, left)
                    } else {
                        break;
                    };
                    used += 3 + width(&fact);
                    spans.push(separator.clone());
                    spans.push(Span::raw(fact));
                }
            }
        }
        frame.render_widget(
            Paragraph::new(Line::from(spans)).style(Style::new().fg(p.text_2).bg(p.ink_850)),
            area,
        );
    }

    /// The log, anchored to the bottom and scrolled back as far as asked, or as far as it goes:
    /// until the oldest row's first line is at the top. Returns how far back that is, in lines.
    fn draw_log(&self, frame: &mut Frame<'_>, area: Rect, p: &Palette) -> usize {
        let (height, width) = (usize::from(area.height), usize::from(area.width));
        self.page.set(height);
        self.log_width.set(width);
        let total: usize = self.rows.iter().map(|entry| entry.lines(width)).sum();
        let furthest = total.saturating_sub(height);
        self.furthest.set(furthest);
        let scroll = self.scroll.min(furthest);
        // The lines shown, counted up from the newest: a row taller than the log shows in part.
        let shown = scroll..scroll + height;
        let mut lines = Vec::with_capacity(height);
        let mut below = 0;
        for entry in self.rows.iter().rev() {
            if below >= shown.end {
                break;
            }
            let count = entry.lines(width);
            if below + count > shown.start {
                let row = row_lines(&entry.row, width, p);
                debug_assert_eq!(row.len(), count, "{:?}", entry.row);
                for (at, line) in (below..).zip(row.into_iter().rev()) {
                    if shown.contains(&at) {
                        lines.push(line);
                    }
                }
            }
            below += count;
        }
        lines.resize(height, Line::default());
        lines.reverse();
        frame.render_widget(
            Paragraph::new(lines).style(Style::new().fg(p.text_2).bg(p.ink_950)),
            area,
        );
        scroll
    }

    /// The palette: a square list of the commands that match, their names aligned with the line
    /// being typed, scrolled to keep the chosen one in sight.
    fn draw_palette(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        p: &Palette,
        listed: &[&CommandHelp],
    ) {
        if area.height == 0 || listed.is_empty() {
            return;
        }
        frame.render_widget(Paragraph::new("").style(Style::new().bg(p.ink_950)), area);
        let usages: Vec<_> = listed.iter().map(|c| clean(&c.usage)).collect();
        let abouts: Vec<_> = listed.iter().map(|c| clean(&c.about)).collect();
        let usage_width = usages.iter().map(|u| width(u)).max().unwrap_or(0);
        let about_width = abouts.iter().map(|a| width(a)).max().unwrap_or(0);
        // A column of padding each side, so the names start where the typed line does.
        let list_at = LINE_AT - 1;
        let list_width = (1 + usage_width + 2 + about_width + 1)
            .min(usize::from(area.width).saturating_sub(list_at + 1));
        let rows = usize::from(area.height);
        let selected = self.selected.min(listed.len() - 1);
        let first = selected.saturating_sub(rows - 1);
        for (y, i) in (area.y..).zip(first..listed.len().min(first + rows)) {
            let mut style = Style::new().bg(if i == selected { p.ink_800 } else { p.ink_850 });
            if i == selected && p.colours == Colours::None {
                style = style.add_modifier(Modifier::REVERSED);
            }
            let pad = " ".repeat(usage_width - width(&usages[i]) + 2);
            let line = Line::from(vec![
                Span::raw(" "),
                Span::styled(usages[i].as_ref(), Style::new().fg(p.text)),
                Span::raw(pad),
                Span::styled(abouts[i].as_ref(), Style::new().fg(p.muted)),
            ]);
            let row = Rect::new(area.x + list_at as u16, y, list_width as u16, 1);
            frame.render_widget(Paragraph::new(line).style(style), row);
        }
    }

    /// The prompt: a gold `>`, the line being typed with a block cursor, and the keys at the right
    /// while there's room for them. A line too long for the row shows its end, where the cursor is.
    fn draw_prompt(&self, frame: &mut Frame<'_>, area: Rect, p: &Palette) {
        let ground = Style::new().fg(p.text).bg(p.ink_900);
        frame.render_widget(Paragraph::new("").style(ground), area);
        let (typed, rest) = self.input.split_at(self.cursor);
        let mut rest = rest.chars();
        let under = rest.next().map_or_else(
            || " ".to_owned(),
            |c| clean(c.encode_utf8(&mut [0; 4])).into_owned(),
        );
        let (typed, rest) = (clean(typed), clean(rest.as_str()));
        let mut room = usize::from(area.width).saturating_sub(LINE_AT + 1);
        let hint = width(&typed) + width(&under) + width(&rest) + 2 + width(HINT) <= room;
        if hint {
            room -= 2 + width(HINT);
        }
        // Keep the cursor in sight: drop characters from the start of the line before it.
        let keep = room.saturating_sub(width(&under));
        let mut from = typed.len();
        let mut used = 0;
        for (i, c) in typed.char_indices().rev() {
            used += char_width(c);
            if used > keep {
                break;
            }
            from = i;
        }
        let cursor = if p.colours == Colours::None {
            Style::new().add_modifier(Modifier::REVERSED)
        } else {
            Style::new().fg(p.ink_900).bg(p.text)
        };
        let line = Line::from(vec![
            Span::raw(" "),
            Span::styled(">", Style::new().fg(p.gold)),
            Span::raw(" "),
            Span::raw(&typed[from..]),
            Span::styled(under, cursor),
            Span::raw(rest),
        ]);
        let row = Rect {
            width: area.width.min((LINE_AT + room) as u16),
            ..area
        };
        frame.render_widget(Paragraph::new(line).style(ground), row);
        if hint {
            at_right(frame, area, HINT, Style::new().fg(p.faint), 0);
        }
    }
}

/// The hairline above the prompt, with how many newer rows wait below while the log is scrolled
/// back.
fn draw_hairline(frame: &mut Frame<'_>, area: Rect, p: &Palette, scroll: usize) {
    let ground = Style::new().fg(p.ink_800).bg(p.ink_900);
    frame.render_widget(
        Paragraph::new("─".repeat(usize::from(area.width))).style(ground),
        area,
    );
    if scroll > 0 {
        let lines = if scroll == 1 { "line" } else { "lines" };
        let note = format!(" {scroll} newer {lines} below · PgDn ");
        at_right(frame, area, &note, Style::new().fg(p.text_2), 0);
    }
}

/// `text` at the right of the row `area`, a column in from its edge, if it fits after the first
/// `taken` columns.
fn at_right(frame: &mut Frame<'_>, area: Rect, text: &str, style: Style, taken: usize) {
    let w = width(text);
    if taken + w + 1 > usize::from(area.width) {
        return;
    }
    let x = area.right() - 1 - w as u16;
    frame.render_widget(
        Paragraph::new(Span::styled(text, style)),
        Rect::new(x, area.y, w as u16, 1),
    );
}

/// A row of the log as the lines it takes at `width` columns.
fn row_lines(row: &Row, width: usize, p: &Palette) -> Vec<Line<'static>> {
    let bold = Style::new().add_modifier(Modifier::BOLD);
    match row {
        Row::Log(line) => {
            // Without colours, WARN and ERROR lines are bold instead.
            let loud = matches!(line.level, Level::Warn | Level::Error);
            let base = if loud && p.colours == Colours::None {
                bold
            } else {
                Style::new()
            };
            let (level, message) = match line.level {
                Level::Error => (p.danger, p.text),
                Level::Warn => (p.warn, p.text),
                Level::Info => (p.muted, p.text_2),
                Level::Debug | Level::Trace => (p.faint, p.text_2),
            };
            let head = vec![
                Span::raw(" "),
                Span::styled(line.time(), base.fg(p.faint)),
                Span::raw(" "),
                Span::styled(format!("{:<5}", line.level.as_str()), base.fg(level)),
                Span::raw(" "),
            ];
            let parts = line
                .parts
                .iter()
                .map(|part| {
                    let colour = if part.name { p.gold } else { message };
                    (clean(&part.text).into_owned(), base.fg(colour))
                })
                .collect();
            wrap(head, MESSAGE_AT, parts, width, LongWords::Indented)
        }
        Row::Command(text) => {
            let head = vec![
                Span::raw(" "),
                Span::styled(">", Style::new().fg(p.gold)),
                Span::raw(" "),
            ];
            let text = (clean(text).into_owned(), Style::new().fg(p.text));
            wrap(head, LINE_AT, vec![text], width, LongWords::Indented)
        }
        Row::Answer { ok, text } => {
            let style = match (ok, p.colours) {
                (true, _) => Style::new().fg(p.text),
                (false, Colours::None) => bold,
                (false, _) => Style::new().fg(p.danger),
            };
            let head = vec![Span::raw(" ".repeat(LINE_AT))];
            let text = vec![(clean(text).into_owned(), style)];
            wrap(head, LINE_AT, text, width, LongWords::AtTheEdge)
        }
        Row::Note(text) => {
            let head = vec![Span::raw(" ".repeat(MESSAGE_AT))];
            let text = (clean(text).into_owned(), Style::new().fg(p.faint));
            wrap(head, MESSAGE_AT, vec![text], width, LongWords::Indented)
        }
    }
}

/// How many lines a row takes at `width` columns: as many as [`row_lines`] draws, from the same
/// text and the same breaks.
fn row_height(row: &Row, width: usize) -> usize {
    let (indent, text) = match row {
        Row::Log(line) => (MESSAGE_AT, line.message()),
        Row::Command(text) | Row::Answer { text, .. } => (LINE_AT, clean(text).into_owned()),
        Row::Note(text) => (MESSAGE_AT, clean(text).into_owned()),
    };
    let widths: Vec<_> = text.chars().map(|c| (c, char_width(c))).collect();
    breaks(&widths, room(indent, width)).len()
}

/// Where the later lines of a word too long for a line start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LongWords {
    /// Under the row's text, as its other later lines do: a log line's, so that none of its lines
    /// can pass for another log line, whatever a player or an op wrote in it (a security review
    /// of the link fix).
    Indented,
    /// At the edge: an answer to the host's own command, whose only words that long are the
    /// server's links, so that a link copied from the screen holds only line breaks, which
    /// launchers drop, rather than the indent's spaces as well (the hosting plan's final review B,
    /// Important 1).
    AtTheEdge,
}

/// `pieces`, each a text and its style, after `head`, broken into lines of `width` columns
/// ([`breaks`]). The lines after the first are indented to `indent`, `head`'s width, but for one
/// that goes on with a word too long for a line where `long` says it starts at the edge.
fn wrap(
    head: Vec<Span<'static>>,
    indent: usize,
    pieces: Vec<(String, Style)>,
    width: usize,
    long: LongWords,
) -> Vec<Line<'static>> {
    let chars: Vec<(char, Style)> = pieces
        .iter()
        .flat_map(|(text, style)| text.chars().map(|c| (c, *style)))
        .collect();
    let widths: Vec<_> = chars.iter().map(|&(c, _)| (c, char_width(c))).collect();
    let mut head = Some(head);
    let mut ended = None;
    breaks(&widths, room(indent, width))
        .into_iter()
        .map(|range| {
            // A word goes on where the line before ended, as no space was dropped between.
            let goes_on = ended == Some(range.start);
            ended = Some(range.end);
            let mut line = head.take().unwrap_or_else(|| {
                let indent = match (goes_on, long) {
                    (true, LongWords::AtTheEdge) => 0,
                    _ => indent,
                };
                vec![Span::raw(" ".repeat(indent))]
            });
            line.extend(runs(&chars[range]));
            Line::from(line)
        })
        .collect()
}

/// The columns a message has on each line at `width`: what's left after its indent, less one
/// to spare.
fn room(indent: usize, width: usize) -> usize {
    width.saturating_sub(indent + 1).max(1)
}

/// Where a message, each character with the columns it takes, breaks into lines of `room`
/// columns: at a space where there is one, which the break drops, else where the line is full.
/// Each line is a range of the characters; an empty message is one empty line.
fn breaks(chars: &[(char, usize)], room: usize) -> Vec<Range<usize>> {
    let mut lines = Vec::new();
    let (mut start, mut used, mut space) = (0, 0, None);
    let mut i = 0;
    while i < chars.len() {
        let (c, w) = chars[i];
        if used + w <= room || i == start {
            used += w;
            if c == ' ' {
                space = Some(i);
            }
            i += 1;
            continue;
        }
        if c == ' ' {
            // The space that doesn't fit ends the line.
            lines.push(start..i);
            i += 1;
            start = i;
            used = 0;
        } else if let Some(s) = space.filter(|&s| s > start) {
            lines.push(start..s);
            start = s + 1;
            used = chars[start..i].iter().map(|&(_, w)| w).sum();
        } else {
            lines.push(start..i);
            start = i;
            used = 0;
        }
        space = None;
    }
    lines.push(start..chars.len());
    lines
}

/// Characters as spans, one for each run of the same style.
fn runs(chars: &[(char, Style)]) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut text = String::new();
    let mut style = None;
    for &(c, s) in chars {
        if style != Some(s) {
            if let Some(previous) = style {
                spans.push(Span::styled(std::mem::take(&mut text), previous));
            }
            style = Some(s);
        }
        text.push(c);
    }
    if let Some(last) = style {
        spans.push(Span::styled(text, last));
    }
    spans
}

/// The columns `c` takes, as ratatui counts them.
fn char_width(c: char) -> usize {
    usize::from(c.encode_utf8(&mut [0; 4]).cell_width())
}

/// The columns `text` takes in a terminal, as ratatui counts them: two for a wide character, such
/// as most Chinese ones. The server's lists line up by it.
pub fn width(text: &str) -> usize {
    text.chars().map(char_width).sum()
}

/// `text` cut to `room` columns, an ellipsis last.
fn shortened(text: &str, room: usize) -> String {
    let mut out = String::new();
    let mut used = 1;
    for c in text.chars() {
        used += char_width(c);
        if used > room {
            break;
        }
        out.push(c);
    }
    out.push('…');
    out
}

/// A line's first word, after any spaces.
fn first_word(line: &str) -> &str {
    line.trim_start()
        .split(char::is_whitespace)
        .next()
        .unwrap_or_default()
}

/// Whether the line is its first word still being typed: the palette's cue to open.
fn typing_first_word(line: &str) -> bool {
    let line = line.trim_start();
    !line.is_empty() && !line.contains(char::is_whitespace)
}

/// The start of the character before byte `at`.
fn before(line: &str, at: usize) -> usize {
    line[..at].char_indices().next_back().map_or(0, |(i, _)| i)
}

/// The end of the character at byte `at`.
fn after(line: &str, at: usize) -> usize {
    line[at..].chars().next().map_or(at, |c| at + c.len_utf8())
}

/// A time span as `H:MM:SS`, with the days ahead past one: `2d 3:04:05`.
fn uptime(ms: i64) -> String {
    let secs = ms.max(0) / 1000;
    let (days, h, m, s) = (
        secs / 86_400,
        secs % 86_400 / 3600,
        secs % 3600 / 60,
        secs % 60,
    );
    if days > 0 {
        format!("{days}d {h}:{m:02}:{s:02}")
    } else {
        format!("{h}:{m:02}:{s:02}")
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::theme::{Colours, Palette};
    use crate::wire::{CommandHelp, Level, LogLine, Part, ServerMsg, State, Status};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::{Buffer, Cell};
    use ratatui::style::{Color, Modifier};

    pub(crate) fn help() -> Vec<CommandHelp> {
        [
            ("invite", "invite [label]", "make an invite link"),
            ("invites", "invites", "list invites"),
            ("online", "online", "who is connected"),
            ("stop", "stop", "stop the server"),
        ]
        .map(|(n, u, a)| CommandHelp {
            name: n.into(),
            usage: u.into(),
            about: a.into(),
        })
        .to_vec()
    }

    /// Each row of `buffer` as a string.
    fn rows_of(buffer: &Buffer) -> Vec<String> {
        let area = buffer.area;
        (area.top()..area.bottom())
            .map(|y| {
                (area.left()..area.right())
                    .map(|x| buffer[(x, y)].symbol())
                    .collect()
            })
            .collect()
    }

    /// The first cell, row by row, where `needle` starts.
    fn cell_at<'b>(buffer: &'b Buffer, needle: &str) -> &'b Cell {
        let area = buffer.area;
        let width = needle.chars().count() as u16;
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right().saturating_sub(width - 1) {
                let found: String = (x..x + width).map(|x| buffer[(x, y)].symbol()).collect();
                if found == needle {
                    return &buffer[(x, y)];
                }
            }
        }
        panic!(
            "{needle:?} isn't on the screen:\n{}",
            rows_of(buffer).join("\n")
        );
    }

    /// The colour `needle` is drawn in.
    fn fg_at(buffer: &Buffer, needle: &str) -> Color {
        cell_at(buffer, needle).fg
    }

    /// The colour behind `needle`.
    fn bg_at(buffer: &Buffer, needle: &str) -> Color {
        cell_at(buffer, needle).bg
    }

    fn key(view: &mut View, code: KeyCode, modifiers: KeyModifiers) -> Option<Action> {
        view.on_key(KeyEvent::new(code, modifiers))
    }

    fn type_text(view: &mut View, text: &str) {
        for c in text.chars() {
            assert_eq!(key(view, KeyCode::Char(c), KeyModifiers::NONE), None);
        }
    }

    fn press(view: &mut View, code: KeyCode) -> Option<Action> {
        key(view, code, KeyModifiers::NONE)
    }

    fn ctrl_c(view: &mut View) -> Option<Action> {
        key(view, KeyCode::Char('c'), KeyModifiers::CONTROL)
    }

    /// A log line at midnight of `parts`, each its text and whether it names someone.
    fn log(level: Level, parts: &[(&str, bool)]) -> ServerMsg {
        ServerMsg::Log {
            line: LogLine {
                at_ms: 0,
                level,
                parts: parts
                    .iter()
                    .map(|&(text, name)| Part {
                        text: text.into(),
                        name,
                    })
                    .collect(),
            },
        }
    }

    fn status(state: State, maintenance: Option<&str>) -> ServerMsg {
        ServerMsg::Status {
            status: Status {
                state,
                server: "Moonward".into(),
                version: "0.5.0".into(),
                started_ms: 0,
                online: 3,
                max: 20,
                maintenance: maintenance.map(Into::into),
                gate_port: 7800,
                api_port: 7801,
            },
        }
    }

    /// `view` drawn at midnight on a `width` × `height` terminal.
    fn drawn(view: &View, width: u16, height: u16, colours: Colours) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|f| view.draw(f, &Palette::of(colours), 0))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    #[test]
    fn the_console_draws_its_rows_at_80_by_24() {
        let mut view = View::new("Moonward".into(), help(), Mode::InProcess);
        view.on_server(ServerMsg::Status {
            status: Status {
                state: State::Online,
                server: "Moonward".into(),
                version: "0.5.0".into(),
                started_ms: 0,
                online: 3,
                max: 20,
                maintenance: None,
                gate_port: 7800,
                api_port: 7801,
            },
        });
        view.on_server(ServerMsg::Log {
            line: LogLine {
                at_ms: 3_723_000,
                level: Level::Warn,
                parts: vec![Part {
                    text: "slow tick".into(),
                    name: false,
                }],
            },
        });
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|f| view.draw(f, &Palette::of(Colours::True), 3_723_000))
            .unwrap();
        let rows = rows_of(terminal.backend().buffer()); // the test's helper: each row as a String
        assert!(
            rows[0].starts_with(" fenji-server — Moonward"),
            "{:?}",
            rows[0]
        );
        assert!(
            rows[1].contains("online")
                && rows[1].contains("3 of 20 online")
                && rows[1].contains("1:02:03")
        );
        assert!(
            rows[21].contains("01:02:03")
                && rows[21].contains("WARN")
                && rows[21].contains("slow tick")
        );
        assert!(rows[23].starts_with(" > "));
        assert_eq!(
            fg_at(terminal.backend().buffer(), "WARN"),
            Color::Rgb(0xe8, 0xb0, 0x4a)
        );
    }

    #[test]
    fn hidden_characters_from_players_show_as_escapes() {
        // Review Focus 1.
        let mut view = View::new("F".into(), help(), Mode::InProcess);
        view.on_server(ServerMsg::Log {
            line: LogLine {
                at_ms: 0,
                level: Level::Info,
                parts: vec![
                    Part {
                        text: "chat from".into(),
                        name: false,
                    },
                    Part {
                        text: "Mai\u{1b}]2;owned\u{7}\u{202e}".into(),
                        name: true,
                    },
                ],
            },
        });
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|f| view.draw(f, &Palette::of(Colours::True), 0))
            .unwrap();
        let all = rows_of(terminal.backend().buffer()).concat();
        assert!(all.contains(r"Mai\u{1b}]2;owned\u{7}\u{202e}"), "{all}");
        assert!(!all.contains('\u{1b}'));
    }

    #[test]
    fn the_palette_completes_and_history_recalls() {
        let mut view = View::new("F".into(), help(), Mode::InProcess);
        type_text(&mut view, "inv");
        assert_eq!(view.palette(), vec!["invite", "invites"]);
        press(&mut view, KeyCode::Down);
        press(&mut view, KeyCode::Tab);
        assert_eq!(view.input(), "invites");
        assert_eq!(
            press(&mut view, KeyCode::Enter),
            Some(Action::Run("invites".into()))
        );
        press(&mut view, KeyCode::Up);
        assert_eq!(
            view.input(),
            "invites",
            "with the palette closed, Up recalls"
        );
        assert_eq!(ctrl_c(&mut view), Some(Action::Interrupt));
    }

    #[test]
    fn a_small_terminal_gets_a_note_not_a_crash() {
        let view = View::new("F".into(), help(), Mode::Attached);
        let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
        terminal
            .draw(|f| view.draw(f, &Palette::of(Colours::None), 0))
            .unwrap();
        assert!(
            rows_of(terminal.backend().buffer())
                .concat()
                .contains("bigger")
        );
    }

    #[test]
    fn the_smallest_full_console_is_60_by_15() {
        let view = View::new("F".into(), help(), Mode::InProcess);
        for (width, height) in [(59, 15), (60, 14)] {
            let rows = rows_of(&drawn(&view, width, height, Colours::True));
            assert!(rows.concat().contains("bigger"), "{width} × {height}");
        }
        let rows = rows_of(&drawn(&view, 60, 15, Colours::True));
        assert!(rows[0].starts_with(" fenji-server — F"), "{rows:?}");
        assert!(rows[14].starts_with(" > "), "{rows:?}");
    }

    #[test]
    fn the_console_draws_its_rows_at_120_by_40_with_the_palette_open() {
        let mut view = View::new("Moonward".into(), help(), Mode::Attached);
        let true_colour = Palette::of(Colours::True);
        // Before the server's status: its name alone.
        let rows = rows_of(&drawn(&view, 120, 40, Colours::True));
        assert_eq!(rows[1].trim(), "Moonward");
        view.on_server(status(State::Maintenance, Some("Updating")));
        view.on_server(log(Level::Info, &[("listening", false)]));
        type_text(&mut view, "inv");
        let buffer = drawn(&view, 120, 40, Colours::True);
        let rows = rows_of(&buffer);
        assert!(rows[0].starts_with(" fenji-server — Moonward"));
        assert!(rows[0].contains("attached"), "{:?}", rows[0]);
        assert!(
            rows[1].starts_with(" maintenance · Moonward") && rows[1].contains("Updating"),
            "{:?}",
            rows[1]
        );
        assert!(rows[1].contains("gate 7800") && rows[1].contains("api 7801"));
        assert_eq!(fg_at(&buffer, "maintenance"), true_colour.warn);
        assert_eq!(fg_at(&buffer, "Moonward ·"), true_colour.gold);
        // The palette: two rows above the hairline, the first chosen.
        assert!(rows[35].contains("listening"), "{rows:#?}");
        assert!(rows[36].contains("invite [label]") && rows[36].contains("make an invite link"));
        assert!(rows[37].contains("invites") && rows[37].contains("list invites"));
        assert!(rows[38].contains('─'));
        assert!(rows[39].starts_with(" > inv"));
        assert!(rows[39].contains("Tab completes · ↑ ↓ choose · Enter runs"));
        assert_eq!(bg_at(&buffer, "invite [label]"), true_colour.ink_800);
        assert_eq!(bg_at(&buffer, "list invites"), true_colour.ink_850);
        assert_eq!(fg_at(&buffer, ">"), true_colour.gold);
        // The block cursor, after "inv".
        assert_eq!(buffer[(6, 39)].bg, true_colour.text);
        // Escape closes it, and the log takes its rows back.
        assert_eq!(press(&mut view, KeyCode::Esc), None);
        assert!(view.palette().is_empty());
        let rows = rows_of(&drawn(&view, 120, 40, Colours::True));
        assert!(!rows.concat().contains("make an invite link"));
        assert!(rows[37].contains("listening"), "{rows:#?}");
    }

    #[test]
    fn the_status_strip_shows_whole_facts_and_shortens_only_the_hosts_message() {
        let mut view = View::new("Moonward".into(), help(), Mode::InProcess);
        let message = "Updating the world to the new version: back in about an hour, thank you all";
        view.on_server(status(State::Maintenance, Some(message)));
        let rows = rows_of(&drawn(&view, 60, 15, Colours::True));
        assert_eq!(
            rows[1].trim_end(),
            " maintenance · Moonward · v0.5.0 · up 0:00:00"
        );
        let rows = rows_of(&drawn(&view, 120, 40, Colours::True));
        assert!(
            rows[1].contains("online · Updating the world") && rows[1].trim_end().ends_with('…'),
            "{:?}",
            rows[1]
        );
        assert!(!rows[1].contains("gate"), "{:?}", rows[1]);
    }

    #[test]
    fn the_log_has_its_columns_and_colours() {
        let mut view = View::new("F".into(), help(), Mode::InProcess);
        view.on_server(log(Level::Info, &[("entered ", false), ("Mai", true)]));
        view.on_server(log(Level::Error, &[("boom", false)]));
        let p = Palette::of(Colours::True);
        let buffer = drawn(&view, 80, 24, Colours::True);
        let rows = rows_of(&buffer);
        assert!(
            rows[20].starts_with(" 00:00:00 INFO  entered Mai"),
            "{:?}",
            rows[20]
        );
        assert!(
            rows[21].starts_with(" 00:00:00 ERROR boom"),
            "{:?}",
            rows[21]
        );
        assert_eq!(fg_at(&buffer, "00:00:00"), p.faint);
        assert_eq!(fg_at(&buffer, "INFO"), p.muted);
        assert_eq!(fg_at(&buffer, "entered"), p.text_2);
        assert_eq!(fg_at(&buffer, "Mai"), p.gold);
        assert_eq!(fg_at(&buffer, "ERROR"), p.danger);
        assert_eq!(fg_at(&buffer, "boom"), p.text);
        assert_eq!(bg_at(&buffer, "boom"), p.ink_950);
        assert_eq!(bg_at(&buffer, "fenji-server"), p.ink_900);
    }

    #[test]
    fn a_long_line_wraps_under_its_message() {
        let mut view = View::new("F".into(), help(), Mode::InProcess);
        let words = (0..30)
            .map(|n| format!("w{n:02}"))
            .collect::<Vec<_>>()
            .join(" ");
        view.on_server(log(Level::Info, &[(&words, false)]));
        let rows = rows_of(&drawn(&view, 60, 15, Colours::True));
        // 43 columns for the message: eleven words a row, broken at spaces.
        assert!(rows[10].starts_with(" 00:00:00 INFO  w00 w01"), "{rows:#?}");
        assert!(rows[10].trim_end().ends_with("w10"), "{rows:#?}");
        assert!(
            rows[11].starts_with(&format!("{}w11 w12", " ".repeat(16))),
            "{rows:#?}"
        );
        assert!(rows[12].trim_end().ends_with("w29"), "{rows:#?}");
    }

    #[test]
    fn the_title_says_the_times_are_utc() {
        // The hosting plan's final review B, Minor 1: the log's times are UTC (Ruling HP6), and
        // the console says so once, on the title bar's right, at 80 columns whatever the name.
        let title = |server: &str, mode| {
            let view = View::new(server.into(), help(), mode);
            rows_of(&drawn(&view, 80, 24, Colours::True))[0]
                .trim_end()
                .to_owned()
        };
        let moonward = title("Moonward", Mode::InProcess);
        assert!(
            moonward.ends_with("Ctrl-C stops the server · times in UTC"),
            "{moonward:?}"
        );
        // The longest name leaves room for it said shorter.
        let longest = title(&"M".repeat(32), Mode::InProcess);
        assert!(
            longest.ends_with("Ctrl-C stops the server · UTC"),
            "{longest:?}"
        );
        let attached = title("Moonward", Mode::Attached);
        assert!(
            attached.ends_with("attached · Ctrl-C or exit leaves · times in UTC"),
            "{attached:?}"
        );
    }

    /// What a host copies of `link` from `rows`: from where it starts, row by row, as a terminal
    /// copies a selection, each row without the blanks after its text and a line break between.
    fn copied(rows: &[String], link: &str) -> String {
        // Enough of it to tell links to one server apart.
        let begins = &link[..32];
        let start = rows
            .iter()
            .position(|r| r.contains(begins))
            .unwrap_or_else(|| panic!("{link} isn't on the screen:\n{}", rows.join("\n")));
        let first = &rows[start][rows[start].find(begins).unwrap()..];
        let mut copy = first.trim_end().to_owned();
        for row in &rows[start + 1..] {
            if copy.chars().filter(|c| *c != '\n').count() >= link.chars().count() {
                break;
            }
            copy.push('\n');
            copy.push_str(row.trim_end());
        }
        copy
    }

    #[test]
    fn a_link_too_long_for_its_row_is_copied_whole() {
        // The hosting plan's final review B, Important 1: at 80 columns an invite link wraps, and
        // its later rows were indented, so a copy held the indent's spaces, which the launchers
        // refused. In an answer to the host's own command, `invite`'s or `link`'s, its later rows
        // start at the edge: a copy holds only line breaks, which they drop.
        let invite = "https://192.168.0.12:47901/#invite=PNAT-VNAQ-NYQ7-AK7A&pin=\
                      bbTyFLCoTiZegeDN_cilR1RF7lQWI41rB3wuOlDr9Cw";
        let server = "https://192.168.0.12:47901/#pin=bbTyFLCoTiZegeDN_cilR1RF7lQWI41rB3wuOlDr9Cw";
        let mut view = View::new("F".into(), help(), Mode::InProcess);
        view.on_server(ServerMsg::Answer {
            id: 1,
            ok: true,
            lines: vec![format!("link (enp13s0): {invite}")],
            more: false,
        });
        view.on_server(ServerMsg::Answer {
            id: 2,
            ok: true,
            lines: vec![format!("server link (enp13s0): {server}")],
            more: false,
        });
        for width in [60, 80, 100] {
            let rows = rows_of(&drawn(&view, width, 30, Colours::True));
            for link in [invite, server] {
                let copy = copied(&rows, link);
                assert_eq!(
                    copy.replace('\n', ""),
                    link,
                    "at {width}:\n{}",
                    rows.join("\n")
                );
            }
        }
        // Words broken at their spaces stay under their message.
        let words = (0..30)
            .map(|n| format!("w{n:02}"))
            .collect::<Vec<_>>()
            .join(" ");
        view.on_server(ServerMsg::Answer {
            id: 2,
            ok: true,
            lines: vec![words],
            more: false,
        });
        let rows = rows_of(&drawn(&view, 60, 30, Colours::True));
        let w12 = rows.iter().find(|r| r.contains("w12")).unwrap();
        assert!(w12.starts_with("   w"), "{w12:?}");
    }

    #[test]
    fn a_log_line_can_never_draw_a_row_that_passes_for_another() {
        // A security review of the link fix: an op writes `!announce`'s text, which the world
        // logs as typed. A word that fills a row at 80 columns and ends in a time, then words
        // that look like a level and a message, would draw a forged log line had the word's later
        // row started at the edge. A log line's later rows always start under its message.
        let word = format!("{}12:00:01", "x".repeat(63));
        let mut view = View::new("F".into(), help(), Mode::InProcess);
        view.on_server(log(
            Level::Info,
            &[
                ("an admin's command account=", false),
                ("1", true),
                (" character=", false),
                ("Mai", true),
                (&format!(" command=announce {word} INFO  forged"), false),
            ],
        ));
        for width in [60, 80, 100] {
            let rows = rows_of(&drawn(&view, width, 30, Colours::True));
            let first = rows
                .iter()
                .position(|r| r.contains("an admin's command"))
                .unwrap();
            let last = rows.iter().position(|r| r.contains("forged")).unwrap();
            for row in &rows[first + 1..=last] {
                assert!(
                    row.starts_with(&" ".repeat(MESSAGE_AT)),
                    "at {width}, a row not under its message: {row:?}\n{}",
                    rows.join("\n")
                );
            }
        }
        // At 80 columns the word's last row is the time and the rest: indented, it's no line.
        let rows = rows_of(&drawn(&view, 80, 30, Colours::True));
        let forged = rows.iter().find(|r| r.contains("INFO  forged")).unwrap();
        assert_eq!(
            forged.trim_end(),
            format!("{}12:00:01 INFO  forged", " ".repeat(MESSAGE_AT))
        );
    }

    #[test]
    fn without_colours_warnings_are_bold_and_the_choice_is_reversed() {
        let mut view = View::new("F".into(), help(), Mode::InProcess);
        view.on_server(log(Level::Warn, &[("slow tick", false)]));
        view.on_server(log(Level::Info, &[("fine", false)]));
        type_text(&mut view, "inv");
        let buffer = drawn(&view, 80, 24, Colours::None);
        assert!(
            cell_at(&buffer, "slow tick")
                .modifier
                .contains(Modifier::BOLD)
        );
        assert!(cell_at(&buffer, "WARN").modifier.contains(Modifier::BOLD));
        assert!(!cell_at(&buffer, "fine").modifier.contains(Modifier::BOLD));
        assert!(
            cell_at(&buffer, "invite [label]")
                .modifier
                .contains(Modifier::REVERSED)
        );
        assert!(
            !cell_at(&buffer, "list invites")
                .modifier
                .contains(Modifier::REVERSED)
        );
        assert!(
            buffer[(6, 23)].modifier.contains(Modifier::REVERSED),
            "the cursor"
        );
        let area = buffer.area;
        assert!(
            area.positions()
                .all(|p| buffer[p].fg == Color::Reset && buffer[p].bg == Color::Reset)
        );
    }

    #[test]
    fn page_up_and_page_down_scroll_the_log() {
        let mut view = View::new("F".into(), help(), Mode::InProcess);
        for n in 0..100 {
            view.on_server(log(Level::Info, &[(&format!("line {n}"), false)]));
        }
        assert!(rows_of(&drawn(&view, 80, 24, Colours::True))[21].contains("line 99"));
        // A page is the log's 20 lines, less one kept for context.
        press(&mut view, KeyCode::PageUp);
        let rows = rows_of(&drawn(&view, 80, 24, Colours::True));
        assert!(rows[21].contains("line 80"), "{rows:#?}");
        assert!(rows[22].contains("19 newer lines below"), "{:?}", rows[22]);
        // New lines don't move a log scrolled back.
        view.on_server(log(Level::Info, &[("line 100", false)]));
        assert!(rows_of(&drawn(&view, 80, 24, Colours::True))[21].contains("line 80"));
        press(&mut view, KeyCode::PageDown);
        press(&mut view, KeyCode::PageDown);
        let rows = rows_of(&drawn(&view, 80, 24, Colours::True));
        assert!(rows[21].contains("line 100"), "{rows:#?}");
        assert!(!rows[22].contains("newer"));
        // The oldest line is as far back as it goes.
        for _ in 0..20 {
            press(&mut view, KeyCode::PageUp);
        }
        let rows = rows_of(&drawn(&view, 80, 24, Colours::True));
        assert!(rows[2].contains("line 0 "), "{rows:#?}");
        assert!(rows[21].contains("line 19 "), "{rows:#?}");
    }

    /// The rows whose first line shows in the log at 80 × 24, by their `rowNNN` names.
    fn shown_rows(view: &View) -> Vec<String> {
        rows_of(&drawn(view, 80, 24, Colours::True))[2..22]
            .iter()
            .filter_map(|r| r.find("row").map(|at| r[at..at + 6].to_owned()))
            .collect()
    }

    #[test]
    fn paging_reaches_every_row_when_rows_wrap() {
        // Review Important 1: the scroll, its step and the hairline all count screen lines.
        let mut view = View::new("F".into(), help(), Mode::InProcess);
        for n in 0..100 {
            // 87 characters in the message's 63 columns: two lines each.
            let text = format!("row{n:03} {}", "x ".repeat(40));
            view.on_server(log(Level::Info, &[(&text, false)]));
        }
        let names = |range: std::ops::RangeInclusive<usize>| {
            range.map(|n| format!("row{n:03}")).collect::<Vec<_>>()
        };
        assert_eq!(shown_rows(&view), names(90..=99));
        // A page up keeps the top line of the page before in sight: row 090's first line.
        press(&mut view, KeyCode::PageUp);
        assert_eq!(shown_rows(&view), names(81..=90));
        let rows = rows_of(&drawn(&view, 80, 24, Colours::True));
        assert!(
            rows[2].trim_start().starts_with('x'),
            "row 080's second line: {rows:#?}"
        );
        assert!(rows[22].contains("19 newer lines below"), "{:?}", rows[22]);
        // Every row is on some page, all the way back to the oldest.
        let mut seen: std::collections::BTreeSet<String> = shown_rows(&view).into_iter().collect();
        for _ in 0..20 {
            press(&mut view, KeyCode::PageUp);
            seen.extend(shown_rows(&view));
        }
        assert_eq!(shown_rows(&view), names(0..=9));
        assert_eq!(seen.len(), 91, "{seen:?}");
        // And down again to the newest.
        for _ in 0..20 {
            press(&mut view, KeyCode::PageDown);
        }
        assert_eq!(shown_rows(&view), names(90..=99));
        assert!(!rows_of(&drawn(&view, 80, 24, Colours::True))[22].contains("newer"));
    }

    #[test]
    fn a_rows_height_is_the_lines_it_draws() {
        let p = Palette::of(Colours::True);
        let long = format!(
            "Mai\u{1b}[2J {} 你好 {}",
            "word ".repeat(30),
            "x".repeat(150)
        );
        let rows = [
            Row::Log(LogLine {
                at_ms: 0,
                level: Level::Warn,
                parts: vec![
                    Part {
                        text: "entered ".into(),
                        name: false,
                    },
                    Part {
                        text: long.clone(),
                        name: true,
                    },
                ],
            }),
            Row::Command(long.clone()),
            Row::Answer {
                ok: false,
                text: long.clone(),
            },
            Row::Note(long),
            Row::Note(String::new()),
        ];
        for row in &rows {
            for width in (MIN_WIDTH..=200).step_by(7) {
                let width = usize::from(width);
                assert_eq!(
                    row_height(row, width),
                    row_lines(row, width, &p).len(),
                    "{row:?} at {width}"
                );
            }
        }
    }

    #[test]
    fn a_row_taller_than_the_log_shows_in_part_never_as_a_blank_log() {
        // Review Important 1: the oldest row is far taller than the log's 20 lines.
        let mut view = View::new("F".into(), help(), Mode::InProcess);
        let tall = format!("row000 {}", "y ".repeat(1200));
        view.on_server(log(Level::Info, &[(&tall, false)]));
        for n in 1..60 {
            view.on_server(log(Level::Info, &[(&format!("row{n:03}"), false)]));
        }
        drawn(&view, 80, 24, Colours::True);
        for _ in 0..20 {
            press(&mut view, KeyCode::PageUp);
        }
        // As far back as it goes: the tall row's first line at the top, and its next ones below.
        let rows = rows_of(&drawn(&view, 80, 24, Colours::True));
        assert!(
            rows[2].starts_with(" 00:00:00 INFO  row000 y y"),
            "{rows:#?}"
        );
        assert!(rows[2..22].iter().all(|r| r.contains("y y")), "{rows:#?}");
        // Paging down goes through the rest of it, and the log is never blank.
        for _ in 0..10 {
            press(&mut view, KeyCode::PageDown);
            let rows = rows_of(&drawn(&view, 80, 24, Colours::True));
            assert!(
                rows[2..22].iter().all(|r| !r.trim().is_empty()),
                "{rows:#?}"
            );
        }
        assert_eq!(
            shown_rows(&view),
            (40..60).map(|n| format!("row{n:03}")).collect::<Vec<_>>()
        );
        // The newest row taller than the log shows its last lines.
        let mut view = View::new("F".into(), help(), Mode::InProcess);
        view.on_server(log(
            Level::Info,
            &[(&format!("{}end", "z ".repeat(1200)), false)],
        ));
        let rows = rows_of(&drawn(&view, 80, 24, Colours::True));
        assert!(rows[21].trim_end().ends_with("z end"), "{rows:#?}");
        assert!(
            rows[2].starts_with(&format!("{}z z", " ".repeat(16))),
            "{rows:#?}"
        );
    }

    #[test]
    fn the_log_keeps_5000_lines_and_the_history_100_commands() {
        let mut view = View::new("F".into(), help(), Mode::InProcess);
        for n in 0..5_100 {
            view.on_server(log(Level::Info, &[(&format!("line {n}"), false)]));
        }
        assert_eq!(view.rows.len(), SCROLLBACK);
        assert!(matches!(&view.rows[0].row, Row::Log(l) if l.parts[0].text == "line 100"));
        for n in 0..150 {
            type_text(&mut view, &format!("announce {n}"));
            assert_eq!(
                press(&mut view, KeyCode::Enter),
                Some(Action::Run(format!("announce {n}")))
            );
        }
        for _ in 0..HISTORY {
            press(&mut view, KeyCode::Up);
        }
        assert_eq!(view.input(), "announce 50");
        press(&mut view, KeyCode::Up);
        assert_eq!(view.input(), "announce 50");
        press(&mut view, KeyCode::Down);
        assert_eq!(view.input(), "announce 51");
    }

    #[test]
    fn the_history_walks_back_to_the_line_being_typed() {
        let mut view = View::new("F".into(), help(), Mode::InProcess);
        for line in ["online", "invites"] {
            type_text(&mut view, line);
            press(&mut view, KeyCode::Enter);
        }
        // A first word that no command starts with lists nothing, so ↑ walks the history.
        type_text(&mut view, "annou");
        assert!(view.palette().is_empty());
        press(&mut view, KeyCode::Up);
        assert_eq!(view.input(), "invites");
        assert!(
            view.palette().is_empty(),
            "a recalled line doesn't open the palette"
        );
        press(&mut view, KeyCode::Up);
        assert_eq!(view.input(), "online");
        press(&mut view, KeyCode::Down);
        press(&mut view, KeyCode::Down);
        assert_eq!(view.input(), "annou");
    }

    #[test]
    fn enter_runs_the_line_as_typed_unless_a_command_was_chosen() {
        let mut view = View::new("F".into(), help(), Mode::InProcess);
        // "s" never stops the server by itself.
        type_text(&mut view, "s");
        assert_eq!(view.palette(), vec!["stop"]);
        assert_eq!(
            press(&mut view, KeyCode::Enter),
            Some(Action::Run("s".into()))
        );
        type_text(&mut view, "inv");
        press(&mut view, KeyCode::Down);
        assert_eq!(
            press(&mut view, KeyCode::Enter),
            Some(Action::Run("invites".into()))
        );
        assert_eq!(
            press(&mut view, KeyCode::Enter),
            None,
            "an empty line runs nothing"
        );
    }

    #[test]
    fn tab_opens_the_palette_and_completes_the_first_word() {
        let mut view = View::new("F".into(), help(), Mode::InProcess);
        press(&mut view, KeyCode::Tab);
        assert_eq!(view.palette(), vec!["invite", "invites", "online", "stop"]);
        press(&mut view, KeyCode::Up);
        assert_eq!(view.input(), "", "with the palette open, Up chooses");
        press(&mut view, KeyCode::Esc);
        assert!(view.palette().is_empty());
        type_text(&mut view, "ON");
        assert_eq!(
            view.palette(),
            vec!["online"],
            "the server reads words in any case"
        );
        press(&mut view, KeyCode::Tab);
        assert_eq!(view.input(), "online");
        type_text(&mut view, " x");
        assert!(view.palette().is_empty(), "past the first word, it closes");
        key(&mut view, KeyCode::Char('u'), KeyModifiers::CONTROL);
        type_text(&mut view, "dance");
        assert!(view.palette().is_empty(), "no command matches");
    }

    #[test]
    fn the_line_edits_at_its_cursor() {
        let mut view = View::new("F".into(), help(), Mode::InProcess);
        type_text(&mut view, "kik Mai");
        press(&mut view, KeyCode::Home);
        press(&mut view, KeyCode::Right);
        press(&mut view, KeyCode::Right);
        type_text(&mut view, "c");
        assert_eq!(view.input(), "kick Mai");
        press(&mut view, KeyCode::End);
        press(&mut view, KeyCode::Backspace);
        press(&mut view, KeyCode::Left);
        press(&mut view, KeyCode::Delete);
        assert_eq!(view.input(), "kick M");
        key(&mut view, KeyCode::Char('u'), KeyModifiers::CONTROL);
        assert_eq!(view.input(), "");
    }

    #[test]
    fn the_line_takes_no_hidden_characters_and_no_more_than_fits_a_console_line() {
        let mut view = View::new("F".into(), help(), Mode::InProcess);
        type_text(&mut view, "a\u{202e}b\u{1b}c");
        assert_eq!(view.input(), "abc");
        type_text(&mut view, &"é".repeat(INPUT_MAX));
        assert!(view.input().len() <= INPUT_MAX);
        assert!(view.input().len() > INPUT_MAX - 2);
    }

    #[test]
    fn a_command_shows_with_its_answer() {
        let mut view = View::new("F".into(), help(), Mode::InProcess);
        type_text(&mut view, "online");
        press(&mut view, KeyCode::Enter);
        view.on_server(ServerMsg::Answer {
            id: 1,
            ok: true,
            lines: vec!["nobody is online".into()],
            more: false,
        });
        view.on_server(ServerMsg::Answer {
            id: 2,
            ok: false,
            lines: vec!["no command dance; type help".into()],
            more: false,
        });
        view.on_server(ServerMsg::Skipped { lines: 37 });
        let p = Palette::of(Colours::True);
        let buffer = drawn(&view, 80, 24, Colours::True);
        let rows = rows_of(&buffer);
        assert!(rows[18].starts_with(" > online"), "{rows:#?}");
        assert!(rows[19].starts_with("   nobody is online"), "{rows:#?}");
        assert_eq!(fg_at(&buffer, "nobody"), p.text);
        assert_eq!(fg_at(&buffer, "no command"), p.danger);
        assert!(rows[21].contains("… 37 lines skipped"), "{rows:#?}");
    }

    #[test]
    fn ctrl_c_stops_a_server_in_process_and_leaves_an_attached_one() {
        let mut view = View::new("F".into(), help(), Mode::InProcess);
        assert_eq!(ctrl_c(&mut view), Some(Action::Interrupt));
        assert!(rows_of(&drawn(&view, 80, 24, Colours::True))[21].starts_with(" > stop"));
        let rows = rows_of(&drawn(&view, 80, 24, Colours::True));
        assert!(rows[0].contains("Ctrl-C stops the server"), "{:?}", rows[0]);
        let mut view = View::new("F".into(), help(), Mode::Attached);
        assert_eq!(ctrl_c(&mut view), Some(Action::Interrupt));
        let rows = rows_of(&drawn(&view, 80, 24, Colours::True));
        assert!(!rows.concat().contains("> stop"));
        assert!(rows[0].contains("Ctrl-C or exit leaves"), "{:?}", rows[0]);
        // `exit` leaves an attached console too (spec §3.1), and `quit` as plain lines' does (M4
        // hosting plan, Task 11's carry-forward 1); in-process they're the server's words.
        type_text(&mut view, "exit");
        assert_eq!(press(&mut view, KeyCode::Enter), Some(Action::Interrupt));
        type_text(&mut view, "Quit");
        assert_eq!(press(&mut view, KeyCode::Enter), Some(Action::Interrupt));
        let mut view = View::new("F".into(), help(), Mode::InProcess);
        for word in ["exit", "quit"] {
            type_text(&mut view, word);
            assert_eq!(
                press(&mut view, KeyCode::Enter),
                Some(Action::Run(word.into()))
            );
        }
    }

    #[test]
    fn the_hint_gives_way_to_a_long_line() {
        let mut view = View::new("F".into(), help(), Mode::InProcess);
        let hint = "Tab completes · ↑ ↓ choose · Enter runs";
        assert!(rows_of(&drawn(&view, 80, 24, Colours::True))[23].contains(hint));
        type_text(&mut view, &format!("announce {}", "x".repeat(100)));
        let rows = rows_of(&drawn(&view, 80, 24, Colours::True));
        assert!(!rows[23].contains(hint));
        // The end of the line, where the cursor is, stays in sight.
        assert!(rows[23].trim_end().ends_with("xxx"), "{:?}", rows[23]);
        assert!(!rows[23].contains("announce"), "{:?}", rows[23]);
    }
}
