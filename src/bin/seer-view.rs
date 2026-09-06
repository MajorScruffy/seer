use std::collections::HashMap;
use std::env;
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::process::{Command, ExitCode};

use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph};
use ratatui::{DefaultTerminal, Frame};

const HELP: &str = "\
Side-by-side TUI of a seer flow-diff (lazygit-style)

Usage: seer-view [--help]
       seer-view
       seer-view <REV>
       seer-view <REV1> <REV2>
       seer-view -- [PATH...]
       seer-view <REV> <REV> -- [PATH...]

Always opens on a TTY, including when there is no outline change.
One commit: outline-diff vs its parent. J/K or shift-click: first..last.
 q quit   tab/click panel   j/k move   J/K range   d inline/side   g/G top/bottom
";

const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let rest = args.get(1..).unwrap_or(&[]);
    match rest.first().map(String::as_str) {
        Some("-h" | "--help") => {
            print!("{HELP}");
            return ExitCode::SUCCESS;
        }
        Some("-V" | "--version") => {
            println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        _ => {}
    }

    let paths = paths_from_args(rest);
    let commits = match git_log() {
        Ok(c) => c,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(3);
        }
    };
    if !io::stdout().is_terminal() {
        return print_oneshot(rest);
    }

    let mut app = App::new(paths, commits);
    app.reload();
    match ratatui::run(|terminal| {
        execute!(io::stdout(), EnableMouseCapture)?;
        let result = app.run(terminal);
        let _ = execute!(io::stdout(), DisableMouseCapture);
        result
    }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::from(3)
        }
    }
}

fn print_oneshot(rest: &[String]) -> ExitCode {
    let mut seer_args = vec!["seer".to_string(), "diff".to_string()];
    seer_args.extend(rest.iter().cloned());
    let out = match seer::run(&seer_args) {
        Ok(out) => out,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(err.exit_code() as u8);
        }
    };
    let (arg_left, arg_right) = titles_from_args(rest);
    let mut view = parse_diff(&out.stdout);
    if view.left_title == "old" {
        view.left_title = arg_left;
    }
    if view.right_title == "new" {
        view.right_title = arg_right;
    }
    if view.rows.is_empty() {
        eprintln!("no outline change");
    } else {
        let text = render_side_by_side(&view, terminal_width(), false);
        let _ = io::stdout().lock().write_all(text.as_bytes());
    }
    ExitCode::from(out.exit as u8)
}

fn paths_from_args(rest: &[String]) -> Vec<String> {
    if let Some(i) = rest.iter().position(|a| a == "--") {
        return rest[i + 1..].to_vec();
    }
    rest.iter()
        .filter(|a| Path::new(a).exists())
        .cloned()
        .collect()
}

fn titles_from_args(rest: &[String]) -> (String, String) {
    let mut revs = Vec::new();
    let mut paths = false;
    for a in rest {
        if a == "--" {
            paths = true;
            continue;
        }
        if paths {
            continue;
        }
        if Path::new(a).exists() {
            continue;
        }
        revs.push(a.as_str());
        if revs.len() == 2 {
            break;
        }
    }
    match revs.as_slice() {
        [] => ("HEAD".into(), "WORKTREE".into()),
        [r] => ((*r).into(), "WORKTREE".into()),
        [a, b] => ((*a).into(), (*b).into()),
        _ => ("HEAD".into(), "WORKTREE".into()),
    }
}

fn terminal_width() -> usize {
    env::var("COLUMNS")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&n| n >= 20)
        .unwrap_or(80)
}

fn git_log() -> Result<Vec<Commit>, String> {
    let out = Command::new("git")
        .args(["log", "-n", "200", "--format=%h\t%P\t%s"])
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let msg = err.trim();
        if msg.is_empty() {
            return Err("not a git repository".into());
        }
        if msg.contains("not a git") {
            return Err("not a git repository".into());
        }
        return Err(msg.to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(parse_log_line)
        .collect())
}

fn parse_log_line(line: &str) -> Option<Commit> {
    let mut parts = line.splitn(3, '\t');
    let hash = parts.next()?.to_string();
    if hash.is_empty() {
        return None;
    }
    let parents = parts.next().unwrap_or("");
    let subject = parts.next().unwrap_or("").to_string();
    let parent = parents.split_whitespace().next().map(str::to_string);
    Some(Commit {
        hash,
        parent,
        subject,
    })
}

/// Index 0 is WORKTREE; 1.. are commits newest-first.
fn diff_revs(commits: &[Commit], lo: usize, hi: usize) -> Option<(Vec<String>, String, String)> {
    if lo == hi {
        if lo == 0 {
            return Some((Vec::new(), "HEAD".into(), "WORKTREE".into()));
        }
        let c = commits.get(lo - 1)?;
        let p = c.parent.as_ref()?;
        return Some((vec![p.clone(), c.hash.clone()], p.clone(), c.hash.clone()));
    }
    // lo is newer, hi is older (list is newest-first, 0 = WORKTREE).
    if lo == 0 {
        let c = commits.get(hi - 1)?;
        return Some((vec![c.hash.clone()], c.hash.clone(), "WORKTREE".into()));
    }
    let newer = commits.get(lo - 1)?;
    let older = commits.get(hi - 1)?;
    Some((
        vec![older.hash.clone(), newer.hash.clone()],
        older.hash.clone(),
        newer.hash.clone(),
    ))
}

fn seer_diff(revs: &[String], paths: &[String]) -> Result<String, seer::SeerError> {
    let mut args = vec!["seer".into(), "diff".into()];
    args.extend(revs.iter().cloned());
    if !paths.is_empty() {
        args.push("--".into());
        args.extend(paths.iter().cloned());
    }
    Ok(seer::run(&args)?.stdout)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Commit {
    hash: String,
    parent: Option<String>,
    subject: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Context,
    Change,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    Commits,
    Files,
    Diff,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Row {
    left: String,
    right: String,
    kind: Kind,
    file: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SideBySide {
    left_title: String,
    right_title: String,
    rows: Vec<Row>,
}

fn parse_diff(diff: &str) -> SideBySide {
    let mut left_title = String::from("old");
    let mut right_title = String::from("new");
    let mut pairs: Vec<(Option<String>, Option<String>)> = Vec::new();

    let lines: Vec<&str> = if diff.is_empty() {
        Vec::new()
    } else {
        diff.split_inclusive('\n')
            .map(|l| l.strip_suffix('\n').unwrap_or(l))
            .collect()
    };

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if let Some(name) = line.strip_prefix("--- ") {
            left_title = name.to_string();
            i += 1;
            continue;
        }
        if let Some(name) = line.strip_prefix("+++ ") {
            right_title = name.to_string();
            i += 1;
            continue;
        }
        if line.starts_with("@@") {
            i += 1;
            continue;
        }
        if line.starts_with('-') && !line.starts_with("--- ") {
            let mut removed = Vec::new();
            while i < lines.len() && lines[i].starts_with('-') && !lines[i].starts_with("--- ") {
                removed.push(lines[i][1..].to_string());
                i += 1;
            }
            let mut added = Vec::new();
            while i < lines.len() && lines[i].starts_with('+') && !lines[i].starts_with("+++ ") {
                added.push(lines[i][1..].to_string());
                i += 1;
            }
            let n = removed.len().max(added.len());
            for k in 0..n {
                pairs.push((removed.get(k).cloned(), added.get(k).cloned()));
            }
            continue;
        }
        if line.starts_with('+') && !line.starts_with("+++ ") {
            pairs.push((None, Some(line[1..].to_string())));
            i += 1;
            continue;
        }
        let text = line.strip_prefix(' ').unwrap_or(line);
        pairs.push((Some(text.to_string()), Some(text.to_string())));
        i += 1;
    }

    let mut cur = String::new();
    let rows = pairs
        .into_iter()
        .map(|(l, r)| {
            let left = l.unwrap_or_default();
            let right = r.unwrap_or_default();
            if let Some(f) =
                file_path_in_diff_line(&left).or_else(|| file_path_in_diff_line(&right))
            {
                cur = f.to_string();
            }
            let kind = if left == right {
                Kind::Context
            } else {
                Kind::Change
            };
            Row {
                left,
                right,
                kind,
                file: cur.clone(),
            }
        })
        .collect();

    SideBySide {
        left_title,
        right_title,
        rows,
    }
}

fn file_path_in_diff_line(text: &str) -> Option<&str> {
    let first = text.split_whitespace().next()?;
    let stem = first.split(':').next().unwrap_or(first);
    looks_like_path(stem).then_some(stem)
}

fn looks_like_path(s: &str) -> bool {
    s == "<stdin>"
        || s.contains('/')
        || s.ends_with(".rs")
        || s.ends_with(".java")
        || s.ends_with(".ts")
        || s.ends_with(".tsx")
        || s.ends_with(".mts")
        || s.ends_with(".cts")
}

fn row_file(row: &Row) -> &str {
    if row.file.is_empty() {
        "(outline)"
    } else {
        row.file.as_str()
    }
}

fn changed_files(rows: &[Row]) -> Vec<String> {
    let mut files = Vec::new();
    for row in rows {
        if row.kind != Kind::Change {
            continue;
        }
        let name = row_file(row);
        if !files.iter().any(|f| f == name) {
            files.push(name.to_string());
        }
    }
    files
}

struct App {
    paths: Vec<String>,
    commits: Vec<Commit>,
    log_idx: usize,
    range_anchor: Option<usize>,
    commit_state: ListState,
    view: SideBySide,
    files: Vec<String>,
    file_idx: usize,
    scroll: usize,
    focus: Focus,
    list_state: ListState,
    cache: HashMap<String, SideBySide>,
    status: String,
    commits_area: Rect,
    files_area: Rect,
    diff_area: Rect,
    commit_hits: Vec<(u16, usize)>,
    file_hits: Vec<(u16, usize)>,
    click_start: Option<usize>,
    inline: bool,
}

impl App {
    fn new(paths: Vec<String>, commits: Vec<Commit>) -> Self {
        let mut commit_state = ListState::default();
        commit_state.select(Some(0));
        Self {
            paths,
            commits,
            log_idx: 0,
            range_anchor: None,
            commit_state,
            view: parse_diff(""),
            files: Vec::new(),
            file_idx: 0,
            scroll: 0,
            focus: Focus::Commits,
            list_state: ListState::default(),
            cache: HashMap::new(),
            status: String::new(),
            commits_area: Rect::default(),
            files_area: Rect::default(),
            diff_area: Rect::default(),
            commit_hits: Vec::new(),
            file_hits: Vec::new(),
            click_start: None,
            inline: false,
        }
    }

    fn log_len(&self) -> usize {
        1 + self.commits.len()
    }

    fn selected_label(&self) -> String {
        if self.log_idx == 0 {
            return "WORKTREE".into();
        }
        self.commits
            .get(self.log_idx - 1)
            .map(|c| format!("{} {}", c.hash, c.subject))
            .unwrap_or_default()
    }

    fn range(&self) -> (usize, usize) {
        match self.range_anchor {
            Some(a) => (a.min(self.log_idx), a.max(self.log_idx)),
            None => (self.log_idx, self.log_idx),
        }
    }

    fn set_view(&mut self, mut view: SideBySide, fallback_l: String, fallback_r: String) {
        if view.left_title == "old" {
            view.left_title = fallback_l;
        }
        if view.right_title == "new" {
            view.right_title = fallback_r;
        }
        self.files = changed_files(&view.rows);
        self.file_idx = 0;
        self.scroll = 0;
        self.list_state
            .select((!self.files.is_empty()).then_some(0));
        self.view = view;
    }

    fn reload(&mut self) {
        let (lo, hi) = self.range();
        let Some((revs, left, right)) = diff_revs(&self.commits, lo, hi) else {
            self.status = "root commit — no parent".into();
            self.set_view(parse_diff(""), String::new(), String::new());
            return;
        };
        let key = format!("{revs:?}\0{:?}", self.paths);
        if let Some(view) = self.cache.get(&key).cloned() {
            self.status.clear();
            self.set_view(view, left, right);
            return;
        }
        match seer_diff(&revs, &self.paths) {
            Ok(stdout) => {
                let view = parse_diff(&stdout);
                self.cache.insert(key, view.clone());
                self.status.clear();
                self.set_view(view, left, right);
            }
            Err(err) => {
                self.status = err.to_string();
                self.set_view(parse_diff(""), left, right);
            }
        }
    }

    fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        loop {
            terminal.draw(|f| self.draw(f))?;
            if self.handle_event(event::read()?) {
                return Ok(());
            }
        }
    }

    fn max_scroll(&self, page: usize) -> usize {
        self.view.rows.len().saturating_sub(page.max(1))
    }

    fn select_file(&mut self, idx: usize) {
        if self.files.is_empty() {
            return;
        }
        self.file_idx = idx.min(self.files.len() - 1);
        self.list_state.select(Some(self.file_idx));
        let want = &self.files[self.file_idx];
        self.scroll = self
            .view
            .rows
            .iter()
            .position(|r| row_file(r) == want)
            .unwrap_or(0);
    }

    fn select_log(&mut self, idx: usize, keep_range: bool) {
        let n = self.log_len();
        if n == 0 {
            return;
        }
        let idx = idx.min(n - 1);
        let anchor = if keep_range {
            self.range_anchor.or(Some(self.log_idx))
        } else {
            None
        };
        if self.log_idx == idx && self.range_anchor == anchor {
            self.commit_state.select(Some(idx));
            return;
        }
        self.log_idx = idx;
        self.commit_state.select(Some(self.log_idx));
        self.range_anchor = anchor;
        self.reload();
    }

    fn handle_event(&mut self, ev: Event) -> bool {
        match ev {
            Event::Key(key) => self.handle_key(key),
            Event::Mouse(m) => {
                self.handle_mouse(m);
                false
            }
            _ => false,
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        if key.kind != KeyEventKind::Press {
            return false;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return true;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return true,
            KeyCode::Tab => {
                self.focus = match self.focus {
                    Focus::Commits => Focus::Files,
                    Focus::Files => Focus::Diff,
                    Focus::Diff => Focus::Commits,
                };
            }
            KeyCode::Char('v') => {
                if self.range_anchor.is_some() {
                    self.range_anchor = None;
                } else {
                    self.range_anchor = Some(self.log_idx);
                }
                self.reload();
            }
            KeyCode::Char('J') => {
                if self.range_anchor.is_none() {
                    self.range_anchor = Some(self.log_idx);
                }
                self.move_log(1, true);
            }
            KeyCode::Char('K') => {
                if self.range_anchor.is_none() {
                    self.range_anchor = Some(self.log_idx);
                }
                self.move_log(-1, true);
            }
            KeyCode::Char('d') => self.inline = !self.inline,
            KeyCode::Char('j') | KeyCode::Down => self.move_by(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_by(-1),
            KeyCode::Char('g') | KeyCode::Home => self.move_home(),
            KeyCode::Char('G') | KeyCode::End => self.move_end(),
            KeyCode::PageDown => match self.focus {
                Focus::Diff => self.scroll = self.scroll.saturating_add(20),
                Focus::Commits => self.move_log(10, false),
                Focus::Files => self.move_files(10),
            },
            KeyCode::PageUp => match self.focus {
                Focus::Diff => self.scroll = self.scroll.saturating_sub(20),
                Focus::Commits => self.move_log(-10, false),
                Focus::Files => self.move_files(-10),
            },
            _ => {}
        }
        false
    }

    fn handle_mouse(&mut self, m: MouseEvent) {
        let pos = Position::new(m.column, m.row);
        let shift = m.modifiers.contains(KeyModifiers::SHIFT);
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if self.commits_area.contains(pos) {
                    if let Some(i) = list_index_at_row(&self.commit_hits, m.row) {
                        self.focus = Focus::Commits;
                        self.click_start = Some(i);
                        self.select_log(i, shift);
                    }
                } else if self.files_area.contains(pos) {
                    if let Some(i) = list_index_at_row(&self.file_hits, m.row) {
                        self.focus = Focus::Files;
                        self.select_file(i);
                    }
                } else if self.diff_area.contains(pos) {
                    self.focus = Focus::Diff;
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if self.commits_area.contains(pos) && self.click_start.is_none() {
                    if let Some(i) = list_index_at_row(&self.commit_hits, m.row) {
                        self.focus = Focus::Commits;
                        self.select_log(i, shift);
                    }
                }
                self.click_start = None;
            }
            MouseEventKind::Drag(MouseButton::Left) if shift => {
                if self.commits_area.contains(pos) {
                    if let Some(i) = list_index_at_row(&self.commit_hits, m.row) {
                        self.focus = Focus::Commits;
                        self.select_log(i, true);
                    }
                }
            }
            MouseEventKind::ScrollDown => {
                if self.commits_area.contains(pos) {
                    self.focus = Focus::Commits;
                    self.move_log(1, false);
                } else if self.files_area.contains(pos) {
                    self.focus = Focus::Files;
                    self.move_files(1);
                } else if self.diff_area.contains(pos) {
                    self.focus = Focus::Diff;
                    self.scroll = self.scroll.saturating_add(3);
                }
            }
            MouseEventKind::ScrollUp => {
                if self.commits_area.contains(pos) {
                    self.focus = Focus::Commits;
                    self.move_log(-1, false);
                } else if self.files_area.contains(pos) {
                    self.focus = Focus::Files;
                    self.move_files(-1);
                } else if self.diff_area.contains(pos) {
                    self.focus = Focus::Diff;
                    self.scroll = self.scroll.saturating_sub(3);
                }
            }
            _ => {}
        }
    }

    fn move_log(&mut self, delta: i32, keep_range: bool) {
        let n = self.log_len() as i32;
        if n == 0 {
            return;
        }
        let next = (self.log_idx as i32 + delta).clamp(0, n - 1) as usize;
        self.select_log(next, keep_range);
    }

    fn move_files(&mut self, delta: i32) {
        if self.files.is_empty() {
            return;
        }
        let next = (self.file_idx as i32 + delta).clamp(0, self.files.len() as i32 - 1);
        self.select_file(next as usize);
    }

    fn move_by(&mut self, delta: i32) {
        match self.focus {
            Focus::Commits => self.move_log(delta, false),
            Focus::Files => self.move_files(delta),
            Focus::Diff => {
                if delta > 0 {
                    self.scroll = self.scroll.saturating_add(delta as usize);
                } else {
                    self.scroll = self.scroll.saturating_sub((-delta) as usize);
                }
            }
        }
    }

    fn move_home(&mut self) {
        match self.focus {
            Focus::Commits => self.select_log(0, false),
            Focus::Files => self.select_file(0),
            Focus::Diff => self.scroll = 0,
        }
    }

    fn move_end(&mut self) {
        match self.focus {
            Focus::Commits => {
                let n = self.log_len();
                if n > 0 {
                    self.select_log(n - 1, false);
                }
            }
            Focus::Files => {
                if !self.files.is_empty() {
                    self.select_file(self.files.len() - 1);
                }
            }
            Focus::Diff => self.scroll = usize::MAX / 4,
        }
    }

    fn border(&self, panel: Focus) -> Style {
        if self.focus == panel {
            Style::new().fg(Color::Yellow)
        } else {
            Style::new().fg(Color::Gray)
        }
    }

    fn draw(&mut self, f: &mut Frame) {
        let area = f.area();
        let cols = Layout::horizontal([Constraint::Length(40), Constraint::Min(20)]).split(area);
        let left = Layout::vertical([
            Constraint::Percentage(55),
            Constraint::Min(5),
            Constraint::Length(1),
        ])
        .split(cols[0]);
        let right = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(cols[1]);
        self.commits_area = left[0];
        self.files_area = left[1];
        self.diff_area = right[0];

        let (lo, hi) = self.range();
        let mut commit_items = Vec::with_capacity(self.log_len());
        commit_items.push(commit_list_item("WORKTREE", 0, lo, hi));
        for (i, c) in self.commits.iter().enumerate() {
            let text = format!("{} {}", c.hash, c.subject);
            commit_items.push(commit_list_item(&text, i + 1, lo, hi));
        }
        let commit_block = Block::bordered()
            .title(" commits ")
            .border_style(self.border(Focus::Commits));
        let commit_inner = commit_block.inner(left[0]);
        let commit_list = List::new(commit_items)
            .block(commit_block)
            .highlight_style(
                Style::new()
                    .bg(Color::Yellow)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol(" ");
        f.render_stateful_widget(commit_list, left[0], &mut self.commit_state);
        self.commit_hits =
            visible_list_rows(commit_inner, self.commit_state.offset(), self.log_len());

        let file_block = Block::bordered()
            .title(" files ")
            .border_style(self.border(Focus::Files));
        let file_inner = file_block.inner(left[1]);
        if self.files.is_empty() {
            self.file_hits.clear();
            f.render_widget(
                Paragraph::new("no changed files")
                    .style(Style::new().fg(Color::DarkGray))
                    .block(file_block),
                left[1],
            );
        } else {
            let items: Vec<ListItem> = self
                .files
                .iter()
                .map(|name| ListItem::new(name.as_str()))
                .collect();
            let list = List::new(items)
                .block(file_block)
                .highlight_style(
                    Style::new()
                        .bg(Color::Yellow)
                        .fg(Color::Black)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol(" ");
            f.render_stateful_widget(list, left[1], &mut self.list_state);
            self.file_hits =
                visible_list_rows(file_inner, self.list_state.offset(), self.files.len());
        }

        let page = right[0].height.saturating_sub(2) as usize;
        let max = self.max_scroll(page);
        if self.scroll > max {
            self.scroll = max;
        }
        let rows = &self.view.rows;
        let diff_border = self.border(Focus::Diff);
        let empty_msg = if self.status.is_empty() {
            format!(
                "No outline change\n{} → {}",
                self.view.left_title, self.view.right_title
            )
        } else {
            self.status.clone()
        };
        if self.inline {
            let block = Block::bordered()
                .title(format!(
                    " {} → {}  inline ",
                    self.view.left_title, self.view.right_title
                ))
                .border_style(diff_border);
            if rows.is_empty() {
                f.render_widget(
                    Paragraph::new(empty_msg)
                        .style(Style::new().fg(Color::DarkGray))
                        .centered()
                        .block(block),
                    right[0],
                );
            } else {
                f.render_widget(
                    Paragraph::new(inline_lines(&rows[self.scroll..])).block(block),
                    right[0],
                );
            }
        } else {
            let split =
                Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(right[0]);
            let left_block = Block::bordered()
                .title(format!(" {} ", self.view.left_title))
                .border_style(diff_border);
            let right_block = Block::bordered()
                .title(format!(" {} ", self.view.right_title))
                .border_style(diff_border);
            if rows.is_empty() {
                let empty = Paragraph::new(empty_msg)
                    .style(Style::new().fg(Color::DarkGray))
                    .centered();
                f.render_widget(empty.clone().block(left_block), split[0]);
                f.render_widget(empty.block(right_block), split[1]);
            } else {
                let slice = &rows[self.scroll..];
                f.render_widget(
                    Paragraph::new(column_lines(slice, true)).block(left_block),
                    split[0],
                );
                f.render_widget(
                    Paragraph::new(column_lines(slice, false)).block(right_block),
                    split[1],
                );
            }
        }

        f.render_widget(
            Paragraph::new(" q tab j/k J/K  d inline/side ")
                .style(Style::new().fg(Color::DarkGray)),
            left[2],
        );
        f.render_widget(
            Paragraph::new(format!(
                " {}  {} → {}  ({} lines, {}) ",
                self.selected_label(),
                self.view.left_title,
                self.view.right_title,
                self.view.rows.len(),
                if self.inline { "inline" } else { "side" },
            ))
            .style(Style::new().fg(Color::DarkGray)),
            right[1],
        );
    }
}

fn visible_list_rows(inner: Rect, offset: usize, len: usize) -> Vec<(u16, usize)> {
    let n = usize::from(inner.height).min(len.saturating_sub(offset));
    (0..n).map(|i| (inner.y + i as u16, offset + i)).collect()
}

fn list_index_at_row(hits: &[(u16, usize)], row: u16) -> Option<usize> {
    hits.iter().find(|(y, _)| *y == row).map(|(_, i)| *i)
}

fn commit_list_item<'a>(text: &str, idx: usize, lo: usize, hi: usize) -> ListItem<'a> {
    let mut item = ListItem::new(text.to_string());
    if idx >= lo && idx <= hi && lo != hi {
        item = item.style(Style::new().bg(Color::Indexed(238)));
    }
    item
}

fn inline_plain_text(rows: &[Row]) -> Vec<String> {
    let mut out = Vec::new();
    for row in rows {
        match row.kind {
            Kind::Context => out.push(format!(" {}", row.left)),
            Kind::Change => {
                if !row.left.is_empty() {
                    out.push(format!("-{}", row.left));
                }
                if !row.right.is_empty() {
                    out.push(format!("+{}", row.right));
                }
            }
        }
    }
    out
}

fn inline_lines(rows: &[Row]) -> Vec<Line<'static>> {
    inline_plain_text(rows)
        .into_iter()
        .map(|text| {
            let style = match text.chars().next() {
                Some('-') => Style::new().fg(Color::Red),
                Some('+') => Style::new().fg(Color::Green),
                _ => Style::new(),
            };
            Line::from(Span::styled(text, style))
        })
        .collect()
}

fn column_lines(rows: &[Row], left: bool) -> Vec<Line<'static>> {
    rows.iter()
        .map(|row| {
            let text = if left { &row.left } else { &row.right };
            let style = match row.kind {
                Kind::Context => Style::new(),
                Kind::Change if text.is_empty() => Style::new().fg(Color::DarkGray),
                Kind::Change if left => Style::new().fg(Color::Red),
                Kind::Change => Style::new().fg(Color::Green),
            };
            Line::from(Span::styled(text.clone(), style))
        })
        .collect()
}

fn render_side_by_side(view: &SideBySide, width: usize, color: bool) -> String {
    let col = ((width.saturating_sub(3)) / 2).max(8);
    let mut out = String::new();
    out.push_str(&format_row(
        &view.left_title,
        &view.right_title,
        col,
        color,
        Kind::Context,
    ));
    out.push_str(&format_rule(col, color));
    for row in &view.rows {
        out.push_str(&format_row(&row.left, &row.right, col, color, row.kind));
    }
    out
}

fn format_rule(col: usize, color: bool) -> String {
    let line = format!("{}┄{}┄{}", "┄".repeat(col), "┄", "┄".repeat(col));
    if color {
        format!("{DIM}{line}{RESET}\n")
    } else {
        format!("{line}\n")
    }
}

fn format_row(left: &str, right: &str, col: usize, color: bool, kind: Kind) -> String {
    let l = pad_or_truncate(left, col);
    let r = pad_or_truncate(right, col);
    let (l, r) = if !color {
        (l, r)
    } else {
        match kind {
            Kind::Change => (
                if left.is_empty() {
                    format!("{DIM}{l}{RESET}")
                } else {
                    format!("{RED}{l}{RESET}")
                },
                if right.is_empty() {
                    format!("{DIM}{r}{RESET}")
                } else {
                    format!("{GREEN}{r}{RESET}")
                },
            ),
            Kind::Context => (l, r),
        }
    };
    format!("{l} │ {r}\n")
}

fn pad_or_truncate(s: &str, width: usize) -> String {
    let n = s.chars().count();
    if n > width {
        let take = width.saturating_sub(1);
        let mut out: String = s.chars().take(take).collect();
        out.push('…');
        out
    } else {
        format!("{s}{}", " ".repeat(width - n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit_fixture(hash: &str, parent: Option<&str>, subject: &str) -> Commit {
        Commit {
            hash: hash.into(),
            parent: parent.map(str::to_string),
            subject: subject.into(),
        }
    }

    #[test]
    fn pairs_minus_plus_side_by_side() {
        let diff = "\
--- HEAD
+++ WORKTREE
@@ -1,3 +1,4 @@
 a.rs fn process
   a.rs handle()
-    return
+    if true
+      return
";
        let view = parse_diff(diff);
        let out = render_side_by_side(&view, 40, false);
        assert!(out.contains("HEAD"));
        assert!(out.contains("WORKTREE"));
        assert!(out.contains("a.rs fn process"));
        assert!(out.contains("a.rs handle()"));
        assert!(out.contains("    return"));
        assert!(out.contains("    if true"));
        assert!(out.contains("│"));
        assert!(!out.contains('\u{1b}'));
        assert_eq!(changed_files(&view.rows), vec!["a.rs"]);
        assert_eq!(
            inline_plain_text(&view.rows),
            vec![
                " a.rs fn process",
                "   a.rs handle()",
                "-    return",
                "+    if true",
                "+      return",
            ]
        );
    }

    #[test]
    fn color_wraps_added_and_removed() {
        let diff = "--- a\n+++ b\n-old\n+new\n";
        let view = parse_diff(diff);
        let out = render_side_by_side(&view, 40, true);
        assert!(out.contains(RED));
        assert!(out.contains(GREEN));
        assert!(out.contains("old"));
        assert!(out.contains("new"));
    }

    #[test]
    fn empty_diff_has_no_files() {
        let view = parse_diff("");
        assert!(view.rows.is_empty());
        assert!(changed_files(&view.rows).is_empty());
    }

    #[test]
    fn parse_log_line_parent_and_root() {
        let c = parse_log_line("abc\tdef123 ghi\tadd tree").unwrap();
        assert_eq!(c.hash, "abc");
        assert_eq!(c.parent.as_deref(), Some("def123"));
        assert_eq!(c.subject, "add tree");
        let root = parse_log_line("aaa\t\tinit").unwrap();
        assert_eq!(root.parent, None);
        assert_eq!(root.subject, "init");
    }

    #[test]
    fn diff_revs_worktree_one_commit_and_range() {
        let commits = vec![
            commit_fixture("c1", Some("c2"), "head"),
            commit_fixture("c2", Some("c3"), "mid"),
            commit_fixture("c3", None, "root"),
        ];
        let wt = diff_revs(&commits, 0, 0).unwrap();
        assert!(wt.0.is_empty());
        assert_eq!((wt.1.as_str(), wt.2.as_str()), ("HEAD", "WORKTREE"));

        let one = diff_revs(&commits, 1, 1).unwrap();
        assert_eq!(one.0, vec!["c2", "c1"]);
        assert_eq!(one.1, "c2");

        let range = diff_revs(&commits, 1, 2).unwrap();
        assert_eq!(range.0, vec!["c2", "c1"]);

        let vs_wt = diff_revs(&commits, 0, 2).unwrap();
        assert_eq!(vs_wt.0, vec!["c2"]);
        assert_eq!(vs_wt.2, "WORKTREE");

        assert!(diff_revs(&commits, 3, 3).is_none());
    }

    #[test]
    fn rows_for_maps_visible_items() {
        let inner = Rect::new(1, 1, 18, 4);
        let hits = visible_list_rows(inner, 3, 10);
        assert_eq!(hits, vec![(1, 3), (2, 4), (3, 5), (4, 6)]);
        assert_eq!(list_index_at_row(&hits, 1), Some(3));
        assert_eq!(list_index_at_row(&hits, 0), None);
        assert_eq!(list_index_at_row(&hits, 5), None);
        assert!(visible_list_rows(inner, 9, 10).len() == 1);
        assert!(visible_list_rows(inner, 10, 10).is_empty());
    }

    #[test]
    fn plus_hunk_is_visible_under_file_filter() {
        let diff = "\
--- a
+++ b
@@ -0,0 +1,3 @@
+src/bin/seer-view.rs fn main
+  env::args()
+  ExitCode::SUCCESS
";
        let view = parse_diff(diff);
        assert!(!view.rows.is_empty());
        assert_eq!(changed_files(&view.rows), vec!["src/bin/seer-view.rs"]);
        let want = "src/bin/seer-view.rs";
        let n = view
            .rows
            .iter()
            .filter(|r| {
                let name = if r.file.is_empty() {
                    "(outline)"
                } else {
                    r.file.as_str()
                };
                name == want
            })
            .count();
        assert_eq!(n, 3);
    }

    #[test]
    fn selecting_first_commit_loads_outline_diff() {
        let _g = GIT_CWD.lock().unwrap_or_else(|e| e.into_inner());
        let Ok(commits) = git_log() else {
            return;
        };
        if commits.is_empty() {
            return;
        }
        let mut app = App::new(Vec::new(), commits);
        app.select_log(1, false);
        assert!(
            !app.view.rows.is_empty() || !app.status.is_empty(),
            "empty commit view: status={:?} titles={}/{} files={}",
            app.status,
            app.view.left_title,
            app.view.right_title,
            app.files.len()
        );
        if !app.view.rows.is_empty() {
            assert!(
                !app.files.is_empty() || app.view.rows.iter().all(|r| r.kind == Kind::Context),
                "diff rows hidden by file filter: files={:?} n_rows={}",
                app.files,
                app.view.rows.len()
            );
        }
    }

    #[test]
    fn click_rendered_commit_row_selects_it() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut app = App::new(
            Vec::new(),
            vec![
                commit_fixture("aaa", Some("bbb"), "one"),
                commit_fixture("ccc", Some("aaa"), "two"),
            ],
        );
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        let (row, idx) = app
            .commit_hits
            .iter()
            .copied()
            .find(|(_, i)| *i == 1)
            .expect("first commit row");
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: app.commits_area.x + 2,
            row,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.log_idx, idx);
    }

    static GIT_CWD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn git(dir: &std::path::Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("run git");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn write_src(dir: &std::path::Path, body: &str) {
        std::fs::write(dir.join("src.rs"), body).unwrap();
        git(dir, &["add", "src.rs"]);
        git(dir, &["commit", "-m", body.lines().next().unwrap()]);
    }

    fn outline_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["-c", "init.defaultBranch=main", "init"]);
        git(dir.path(), &["config", "user.email", "seer@example.com"]);
        git(dir.path(), &["config", "user.name", "seer-test"]);
        git(dir.path(), &["config", "commit.gpgsign", "false"]);
        write_src(dir.path(), "fn main() {\n    return;\n}\n");
        write_src(
            dir.path(),
            "fn main() {\n    if true {\n        return;\n    }\n}\n",
        );
        write_src(
            dir.path(),
            "fn main() {\n    if true {\n        return;\n    } else {\n        return;\n    }\n}\n",
        );
        write_src(
            dir.path(),
            "fn main() {\n    for _ in 0..1 {\n        return;\n    }\n}\n",
        );
        dir
    }

    fn with_repo(dir: &std::path::Path, f: impl FnOnce()) {
        let _g = GIT_CWD.lock().unwrap_or_else(|e| e.into_inner());
        let prev = env::current_dir().unwrap();
        env::set_current_dir(dir).unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        env::set_current_dir(prev).unwrap();
        result.unwrap();
    }

    fn mouse(app: &App, kind: MouseEventKind, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column: app.commits_area.x + 2,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn y_of_commit_row(app: &App, idx: usize) -> u16 {
        app.commit_hits
            .iter()
            .find(|(_, i)| *i == idx)
            .map(|(row, _)| *row)
            .unwrap_or_else(|| panic!("commit {idx} not visible: {:?}", app.commit_hits))
    }

    fn expected_commit(app: &App, idx: usize) -> SideBySide {
        let (revs, left, right) =
            diff_revs(&app.commits, idx, idx).unwrap_or_else(|| panic!("no revs for commit {idx}"));
        let mut view = parse_diff(&seer_diff(&revs, &[]).expect("seer diff"));
        if view.left_title == "old" {
            view.left_title = left;
        }
        if view.right_title == "new" {
            view.right_title = right;
        }
        view
    }

    fn assert_commit_loaded(app: &App, idx: usize) {
        assert_eq!(app.log_idx, idx, "selected index");
        assert_eq!(app.range_anchor, None, "click became a range");
        assert!(
            app.status.is_empty(),
            "reload error on commit {idx}: {}",
            app.status
        );
        let want = expected_commit(app, idx);
        assert!(
            !want.rows.is_empty(),
            "fixture commit {idx} has no outline-diff"
        );
        assert_eq!(app.view, want);
    }

    #[test]
    fn clicking_several_commits_loads_each_outline_diff() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let dir = outline_repo();
        with_repo(dir.path(), || {
            let commits = git_log().expect("git log");
            assert!(
                commits.len() >= 3,
                "need 3 commits with parents, got {}",
                commits.len()
            );
            let mut app = App::new(Vec::new(), commits);
            let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

            for idx in [1usize, 2, 3] {
                terminal.draw(|f| app.draw(f)).unwrap();
                let row = y_of_commit_row(&app, idx);
                let next = app
                    .commit_hits
                    .iter()
                    .find(|(_, i)| *i == idx + 1)
                    .map(|(r, _)| *r)
                    .unwrap_or(row);
                app.handle_mouse(mouse(&app, MouseEventKind::Down(MouseButton::Left), row));
                app.handle_mouse(mouse(&app, MouseEventKind::Drag(MouseButton::Left), next));
                app.handle_mouse(mouse(&app, MouseEventKind::Up(MouseButton::Left), next));
                assert_commit_loaded(&app, idx);
            }

            terminal.draw(|f| app.draw(f)).unwrap();
            let row = y_of_commit_row(&app, 2);
            app.click_start = None;
            app.handle_mouse(mouse(&app, MouseEventKind::Up(MouseButton::Left), row));
            assert_commit_loaded(&app, 2);
        });
    }
}
