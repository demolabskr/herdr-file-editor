//! In-pane file editor — the fork's one opt-in write capability.
//!
//! The upstream viewer is read-only by constitution; this fork adds a single, explicit exception:
//! `e` opens the selected file in a small editor drawn over the content pane, and **only** an
//! explicit `Ctrl+S` writes it back. Everything here is pure state + file I/O helpers; the
//! controller owns the modal and the presenter draws [`FileEditorView`].
//!
//! Safety rules this module enforces:
//! - Only regular, valid UTF-8 text files under [`MAX_BYTES`] without NUL bytes are opened.
//! - The file is written atomically (temp file in the same directory + rename) to its resolved real
//!   path, so a symlink stays a symlink and permissions are carried over.
//! - A save refuses to silently overwrite a file that changed on disk since it was loaded; the
//!   user must confirm the overwrite or reload.
//! - Line endings (LF / CRLF), a UTF-8 BOM, and the presence of a final newline are preserved.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use unicode_width::UnicodeWidthChar;

/// Largest file the in-pane editor opens. Bigger files go to the external editor (`E`).
pub const MAX_BYTES: u64 = 4 * 1024 * 1024;

/// Display width of a tab stop.
pub const TAB_WIDTH: usize = 4;

/// Cap on remembered undo steps.
const UNDO_LIMIT: usize = 1000;

/// A cursor/text position: line index and UTF-8 byte offset within that line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Pos {
    pub row: usize,
    pub col: usize,
}

impl Pos {
    fn new(row: usize, col: usize) -> Self {
        Pos { row, col }
    }
}

/// Size + mtime of the file as last loaded or saved, used to detect outside changes.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DiskStamp {
    len: u64,
    modified: Option<SystemTime>,
}

fn stamp(path: &Path) -> io::Result<DiskStamp> {
    let meta = fs::metadata(path)?;
    Ok(DiskStamp {
        len: meta.len(),
        modified: meta.modified().ok(),
    })
}

/// One reversible edit.
#[derive(Debug, Clone)]
enum EditKind {
    Insert { at: Pos, text: String },
    Delete { at: Pos, text: String },
}

#[derive(Debug, Clone)]
struct Edit {
    /// Unique per edit; the top entry's id identifies the buffer state (see `modified`).
    id: u64,
    kind: EditKind,
    before: Pos,
    after: Pos,
}

/// A pending yes/no question shown in the status row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confirm {
    /// Closing with unsaved changes.
    CloseUnsaved,
    /// Saving over a file that changed on disk since it was loaded.
    OverwriteExternal,
}

/// What the controller must do after the editor consumed a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorAction {
    /// Nothing changed on screen.
    None,
    /// Repaint.
    Redraw,
    /// The file was written: repaint and refresh git/preview state.
    Saved,
    /// Close the editor (already saved or changes discarded).
    Close,
    /// Save succeeded and the editor should close.
    SavedAndClose,
}

/// Why a file cannot be opened in the in-pane editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenError {
    NotAFile,
    TooLarge(u64),
    Binary,
    NotUtf8,
    Io(String),
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenError::NotAFile => write!(f, "일반 파일이 아닙니다"),
            OpenError::TooLarge(n) => write!(
                f,
                "파일이 너무 큽니다 ({} KiB, 최대 {} KiB) — E로 외부 편집기를 쓰세요",
                n / 1024,
                MAX_BYTES / 1024
            ),
            OpenError::Binary => write!(f, "이진 파일은 편집할 수 없습니다"),
            OpenError::NotUtf8 => write!(f, "UTF-8 텍스트가 아닙니다 — E로 외부 편집기를 쓰세요"),
            OpenError::Io(e) => write!(f, "읽기 실패: {e}"),
        }
    }
}

/// Text format details preserved across a load/save round trip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Format {
    crlf: bool,
    bom: bool,
    final_newline: bool,
}

/// Split file text into lines and detect its format.
fn parse_text(raw: &str) -> (Vec<String>, Format) {
    let bom = raw.starts_with('\u{feff}');
    let body = if bom {
        &raw['\u{feff}'.len_utf8()..]
    } else {
        raw
    };
    let crlf = body.contains("\r\n");
    let final_newline = body.ends_with('\n');
    let body = body.strip_suffix('\n').unwrap_or(body);
    let mut lines: Vec<String> = body
        .split('\n')
        .map(|l| {
            if crlf {
                l.strip_suffix('\r').unwrap_or(l).to_string()
            } else {
                l.to_string()
            }
        })
        .collect();
    if lines.is_empty() {
        lines.push(String::new());
    }
    (
        lines,
        Format {
            crlf,
            bom,
            final_newline,
        },
    )
}

/// Join lines back into file text using the original format.
fn serialize(lines: &[String], format: Format) -> String {
    let eol = if format.crlf { "\r\n" } else { "\n" };
    let mut out = String::new();
    if format.bom {
        out.push('\u{feff}');
    }
    out.push_str(&lines.join(eol));
    if format.final_newline {
        out.push_str(eol);
    }
    out
}

/// The leading indentation unit to insert on `Tab`: a tab when the file indents with tabs,
/// otherwise the smallest space indent found (2..=8), defaulting to four spaces.
fn detect_indent(lines: &[String]) -> String {
    let mut tab_lines = 0usize;
    let mut space_lines = 0usize;
    let mut smallest = usize::MAX;
    for line in lines.iter().take(2000) {
        if line.starts_with('\t') {
            tab_lines += 1;
        } else if line.starts_with(' ') {
            let n = line.chars().take_while(|c| *c == ' ').count();
            if n < line.len() {
                space_lines += 1;
                smallest = smallest.min(n);
            }
        }
    }
    if tab_lines > space_lines {
        "\t".to_string()
    } else if (2..=8).contains(&smallest) {
        " ".repeat(smallest)
    } else {
        " ".repeat(4)
    }
}

/// Display width of one character as the editor draws it. Control characters are drawn as a
/// one-cell placeholder so file content can never emit terminal escape sequences.
pub fn char_width(c: char, col: usize) -> usize {
    if c == '\t' {
        TAB_WIDTH - (col % TAB_WIDTH)
    } else if c.is_control() {
        1
    } else {
        c.width().unwrap_or(0)
    }
}

/// Display column of byte offset `byte` in `line`.
pub fn display_col(line: &str, byte: usize) -> usize {
    let mut col = 0;
    for (i, c) in line.char_indices() {
        if i >= byte {
            break;
        }
        col += char_width(c, col);
    }
    col
}

/// Byte offset in `line` whose display column is closest to (not past) `target`.
fn byte_at_display_col(line: &str, target: usize) -> usize {
    let mut col = 0;
    for (i, c) in line.char_indices() {
        let w = char_width(c, col);
        if col + w > target {
            return i;
        }
        col += w;
    }
    line.len()
}

/// The in-pane editor state for one file.
#[derive(Debug, Clone)]
pub struct FileEditor {
    /// The path as selected in the tree (shown in the title).
    path: PathBuf,
    /// The resolved real path that is read and written (follows symlinks).
    real_path: PathBuf,
    /// Root-relative display name.
    title: String,
    lines: Vec<String>,
    format: Format,
    indent: String,
    cursor: Pos,
    /// Remembered display column for vertical movement.
    want_col: Option<usize>,
    /// First visible line.
    top: usize,
    /// First visible display column.
    left: usize,
    /// Text area size (columns, rows) as last drawn.
    viewport: (u16, u16),
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    /// Whether the last undo entry may still absorb typed characters.
    group_open: bool,
    /// Source of [`Edit::id`].
    next_id: u64,
    /// State id (top undo entry's id, 0 = unedited) at the last load/save.
    saved_id: u64,
    disk: DiskStamp,
    confirm: Option<Confirm>,
    message: Option<(String, bool)>,
}

impl FileEditor {
    /// Load `path` for editing. `title` is the root-relative name shown in the frame.
    pub fn open(path: &Path, title: String) -> Result<Self, OpenError> {
        let real_path = fs::canonicalize(path).map_err(|e| OpenError::Io(e.to_string()))?;
        let meta = fs::metadata(&real_path).map_err(|e| OpenError::Io(e.to_string()))?;
        if !meta.is_file() {
            return Err(OpenError::NotAFile);
        }
        if meta.len() > MAX_BYTES {
            return Err(OpenError::TooLarge(meta.len()));
        }
        let bytes = fs::read(&real_path).map_err(|e| OpenError::Io(e.to_string()))?;
        if bytes.contains(&0) {
            return Err(OpenError::Binary);
        }
        let raw = String::from_utf8(bytes).map_err(|_| OpenError::NotUtf8)?;
        let disk = stamp(&real_path).map_err(|e| OpenError::Io(e.to_string()))?;
        Ok(Self::from_text(
            path.to_path_buf(),
            real_path,
            title,
            &raw,
            disk,
        ))
    }

    fn from_text(
        path: PathBuf,
        real_path: PathBuf,
        title: String,
        raw: &str,
        disk: DiskStamp,
    ) -> Self {
        let (lines, format) = parse_text(raw);
        let indent = detect_indent(&lines);
        FileEditor {
            path,
            real_path,
            title,
            lines,
            format,
            indent,
            cursor: Pos::default(),
            want_col: None,
            top: 0,
            left: 0,
            viewport: (80, 20),
            undo: Vec::new(),
            redo: Vec::new(),
            group_open: false,
            next_id: 1,
            saved_id: 0,
            disk,
            confirm: None,
            message: None,
        }
    }

    // ── accessors ────────────────────────────────────────────────────────────────────────────

    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn title(&self) -> &str {
        &self.title
    }
    pub fn lines(&self) -> &[String] {
        &self.lines
    }
    pub fn cursor(&self) -> Pos {
        self.cursor
    }
    pub fn top(&self) -> usize {
        self.top
    }
    pub fn left(&self) -> usize {
        self.left
    }
    pub fn modified(&self) -> bool {
        self.state_id() != self.saved_id
    }
    fn state_id(&self) -> u64 {
        self.undo.last().map_or(0, |e| e.id)
    }
    pub fn confirm(&self) -> Option<Confirm> {
        self.confirm
    }
    pub fn message(&self) -> Option<(&str, bool)> {
        self.message.as_ref().map(|(m, e)| (m.as_str(), *e))
    }
    pub fn crlf(&self) -> bool {
        self.format.crlf
    }
    pub fn text(&self) -> String {
        serialize(&self.lines, self.format)
    }

    /// Jump to a 1-based line (used when the viewer already scrolled to a spot).
    pub fn goto_line(&mut self, line: usize) {
        let row = line.saturating_sub(1).min(self.lines.len() - 1);
        self.cursor = Pos::new(row, 0);
        self.top = row.saturating_sub(self.viewport.1 as usize / 3);
        self.ensure_visible();
    }

    /// Record the drawn text-area size and keep the cursor on screen. Returns whether it changed.
    pub fn set_viewport(&mut self, cols: u16, rows: u16) -> bool {
        let next = (cols.max(1), rows.max(1));
        if next == self.viewport {
            return false;
        }
        self.viewport = next;
        self.ensure_visible();
        true
    }

    fn ensure_visible(&mut self) {
        let rows = self.viewport.1.max(1) as usize;
        let cols = self.viewport.0.max(1) as usize;
        if self.cursor.row < self.top {
            self.top = self.cursor.row;
        } else if self.cursor.row >= self.top + rows {
            self.top = self.cursor.row + 1 - rows;
        }
        let dcol = display_col(&self.lines[self.cursor.row], self.cursor.col);
        if dcol < self.left {
            self.left = dcol;
        } else if dcol >= self.left + cols {
            self.left = dcol + 1 - cols;
        }
    }

    /// Scroll the view by `delta` lines without moving the text (mouse wheel). The cursor is
    /// dragged along only if it would leave the screen.
    pub fn scroll(&mut self, delta: isize) {
        let max_top = self.lines.len().saturating_sub(1);
        self.top = (self.top as isize + delta).clamp(0, max_top as isize) as usize;
        let rows = self.viewport.1.max(1) as usize;
        if self.cursor.row < self.top {
            self.move_to_row(self.top);
        } else if self.cursor.row >= self.top + rows {
            self.move_to_row(self.top + rows - 1);
        }
    }

    /// Place the cursor at a text-area cell (mouse click), relative to the text area's origin.
    pub fn click(&mut self, x: u16, y: u16) {
        let row = (self.top + y as usize).min(self.lines.len() - 1);
        let col = byte_at_display_col(&self.lines[row], self.left + x as usize);
        self.cursor = Pos::new(row, col);
        self.want_col = None;
        self.group_open = false;
        self.ensure_visible();
    }

    fn move_to_row(&mut self, row: usize) {
        let row = row.min(self.lines.len() - 1);
        let want = self
            .want_col
            .unwrap_or_else(|| display_col(&self.lines[self.cursor.row], self.cursor.col));
        self.cursor = Pos::new(row, byte_at_display_col(&self.lines[row], want));
        self.want_col = Some(want);
    }

    // ── text primitives ──────────────────────────────────────────────────────────────────────

    /// Insert `text` (may contain `\n`) at `at`; returns the position just after it.
    fn raw_insert(&mut self, at: Pos, text: &str) -> Pos {
        let line = &self.lines[at.row];
        let head = line[..at.col].to_string();
        let tail = line[at.col..].to_string();
        let parts: Vec<&str> = text.split('\n').collect();
        if parts.len() == 1 {
            self.lines[at.row] = format!("{head}{text}{tail}");
            return Pos::new(at.row, at.col + text.len());
        }
        self.lines[at.row] = format!("{head}{}", parts[0]);
        let last = parts[parts.len() - 1];
        let mut new_rows: Vec<String> = parts[1..parts.len() - 1]
            .iter()
            .map(|s| s.to_string())
            .collect();
        new_rows.push(format!("{last}{tail}"));
        let end = Pos::new(at.row + parts.len() - 1, last.len());
        self.lines.splice(at.row + 1..at.row + 1, new_rows);
        end
    }

    /// Delete the text between `start` and `end` (start <= end); returns the removed text.
    fn raw_delete(&mut self, start: Pos, end: Pos) -> String {
        if start.row == end.row {
            return self.lines[start.row].drain(start.col..end.col).collect();
        }
        let mut removed = self.lines[start.row][start.col..].to_string();
        for row in start.row + 1..end.row {
            removed.push('\n');
            removed.push_str(&self.lines[row]);
        }
        removed.push('\n');
        removed.push_str(&self.lines[end.row][..end.col]);
        let tail = self.lines[end.row][end.col..].to_string();
        self.lines[start.row].truncate(start.col);
        self.lines[start.row].push_str(&tail);
        self.lines.drain(start.row + 1..=end.row);
        removed
    }

    fn end_of(at: Pos, text: &str) -> Pos {
        match text.rfind('\n') {
            None => Pos::new(at.row, at.col + text.len()),
            Some(i) => Pos::new(at.row + text.matches('\n').count(), text.len() - i - 1),
        }
    }

    fn push_undo(&mut self, kind: EditKind, before: Pos, after: Pos) {
        let id = self.next_id;
        self.next_id += 1;
        self.undo.push(Edit {
            id,
            kind,
            before,
            after,
        });
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    // ── edits ────────────────────────────────────────────────────────────────────────────────

    fn insert_text(&mut self, text: &str, groupable: bool) {
        let before = self.cursor;
        let after = self.raw_insert(before, text);
        let merged = groupable
            && self.group_open
            && matches!(
                self.undo.last(),
                Some(Edit { kind: EditKind::Insert { .. }, after: a, .. }) if *a == before
            );
        if merged {
            if let Some(Edit {
                kind: EditKind::Insert { text: t, .. },
                after: a,
                ..
            }) = self.undo.last_mut()
            {
                t.push_str(text);
                *a = after;
            }
            self.redo.clear();
        } else {
            self.push_undo(
                EditKind::Insert {
                    at: before,
                    text: text.to_string(),
                },
                before,
                after,
            );
        }
        self.group_open = groupable;
        self.cursor = after;
        self.want_col = None;
    }

    fn delete_range(&mut self, start: Pos, end: Pos) {
        if start == end {
            return;
        }
        let before = self.cursor;
        let text = self.raw_delete(start, end);
        self.push_undo(EditKind::Delete { at: start, text }, before, start);
        self.group_open = false;
        self.cursor = start;
        self.want_col = None;
    }

    fn prev_pos(&self, p: Pos) -> Option<Pos> {
        if p.col > 0 {
            let c = self.lines[p.row][..p.col].chars().next_back()?;
            Some(Pos::new(p.row, p.col - c.len_utf8()))
        } else if p.row > 0 {
            Some(Pos::new(p.row - 1, self.lines[p.row - 1].len()))
        } else {
            None
        }
    }

    fn next_pos(&self, p: Pos) -> Option<Pos> {
        if let Some(c) = self.lines[p.row][p.col..].chars().next() {
            Some(Pos::new(p.row, p.col + c.len_utf8()))
        } else if p.row + 1 < self.lines.len() {
            Some(Pos::new(p.row + 1, 0))
        } else {
            None
        }
    }

    fn backspace(&mut self) {
        if let Some(prev) = self.prev_pos(self.cursor) {
            self.delete_range(prev, self.cursor);
        }
    }

    fn delete_forward(&mut self) {
        if let Some(next) = self.next_pos(self.cursor) {
            self.delete_range(self.cursor, next);
        }
    }

    fn outdent(&mut self) {
        let row = self.cursor.row;
        let line = &self.lines[row];
        let n = if line.starts_with('\t') {
            1
        } else {
            line.chars()
                .take(self.indent.len().max(1))
                .take_while(|c| *c == ' ')
                .count()
        };
        if n == 0 {
            return;
        }
        let cursor = self.cursor;
        self.delete_range(Pos::new(row, 0), Pos::new(row, n));
        self.cursor = Pos::new(row, cursor.col.saturating_sub(n));
    }

    pub fn undo(&mut self) -> bool {
        let Some(edit) = self.undo.pop() else {
            return false;
        };
        match &edit.kind {
            EditKind::Insert { at, text } => {
                let end = Self::end_of(*at, text);
                self.raw_delete(*at, end);
            }
            EditKind::Delete { at, text } => {
                self.raw_insert(*at, text);
            }
        }
        self.cursor = edit.before;
        self.redo.push(edit);
        self.group_open = false;
        self.want_col = None;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(edit) = self.redo.pop() else {
            return false;
        };
        match &edit.kind {
            EditKind::Insert { at, text } => {
                self.raw_insert(*at, text);
            }
            EditKind::Delete { at, text } => {
                let end = Self::end_of(*at, text);
                self.raw_delete(*at, end);
            }
        }
        self.cursor = edit.after;
        self.undo.push(edit);
        self.group_open = false;
        self.want_col = None;
        true
    }

    /// Insert pasted text as one undo step (bracketed paste). CR/CRLF are normalized to `\n`.
    pub fn paste(&mut self, text: &str) {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        if text.is_empty() {
            return;
        }
        self.confirm = None;
        self.message = None;
        self.insert_text(&text, false);
        self.ensure_visible();
    }

    // ── saving ───────────────────────────────────────────────────────────────────────────────

    /// Whether the file on disk differs from what was loaded/saved last.
    pub fn changed_on_disk(&self) -> bool {
        stamp(&self.real_path).map_or(true, |s| s != self.disk)
    }

    /// Write the buffer. Refuses (raising [`Confirm::OverwriteExternal`]) when the file changed
    /// on disk, unless `force`.
    fn save(&mut self, force: bool) -> Result<bool, String> {
        if !force && self.changed_on_disk() {
            self.confirm = Some(Confirm::OverwriteExternal);
            return Ok(false);
        }
        let data = self.text();
        write_atomic(&self.real_path, data.as_bytes()).map_err(|e| e.to_string())?;
        self.disk = stamp(&self.real_path).map_err(|e| e.to_string())?;
        self.saved_id = self.state_id();
        self.group_open = false;
        self.message = Some((format!("저장했습니다 ({}줄)", self.lines.len()), false));
        Ok(true)
    }

    fn reload(&mut self) -> Result<(), String> {
        let fresh = FileEditor::open(&self.path, self.title.clone()).map_err(|e| e.to_string())?;
        let cursor = self.cursor;
        let viewport = self.viewport;
        *self = fresh;
        self.viewport = viewport;
        self.cursor = Pos::new(cursor.row.min(self.lines.len() - 1), 0);
        self.ensure_visible();
        self.message = Some(("디스크의 내용으로 다시 불러왔습니다".to_string(), false));
        Ok(())
    }

    fn try_save(&mut self, force: bool, close_after: bool) -> EditorAction {
        match self.save(force) {
            Ok(true) => {
                self.confirm = None;
                if close_after {
                    EditorAction::SavedAndClose
                } else {
                    EditorAction::Saved
                }
            }
            Ok(false) => EditorAction::Redraw,
            Err(e) => {
                self.confirm = None;
                self.message = Some((format!("저장 실패: {e}"), true));
                EditorAction::Redraw
            }
        }
    }

    // ── keys ─────────────────────────────────────────────────────────────────────────────────

    /// Handle one key press.
    pub fn handle_key(&mut self, key: KeyEvent) -> EditorAction {
        if let Some(confirm) = self.confirm {
            return self.handle_confirm_key(confirm, key);
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let had_message = self.message.take().is_some();

        if ctrl {
            let action = match key.code {
                KeyCode::Char('s') | KeyCode::Char('S') => return self.try_save(false, false),
                KeyCode::Char('z') if shift => self.redo(),
                KeyCode::Char('Z') => self.redo(),
                KeyCode::Char('z') => self.undo(),
                KeyCode::Char('y') | KeyCode::Char('Y') => self.redo(),
                KeyCode::Char('q') | KeyCode::Char('Q') => return self.request_close(),
                KeyCode::Home => {
                    self.cursor = Pos::default();
                    self.want_col = None;
                    self.group_open = false;
                    true
                }
                KeyCode::End => {
                    let row = self.lines.len() - 1;
                    self.cursor = Pos::new(row, self.lines[row].len());
                    self.want_col = None;
                    self.group_open = false;
                    true
                }
                _ => false,
            };
            self.ensure_visible();
            return if action || had_message {
                EditorAction::Redraw
            } else {
                EditorAction::None
            };
        }

        let page = self.viewport.1.max(2) as usize - 1;
        match key.code {
            KeyCode::Esc => return self.request_close(),
            KeyCode::Char(c) if !alt => self.insert_text(&c.to_string(), true),
            KeyCode::Enter => self.insert_text("\n", false),
            KeyCode::Tab => {
                let indent = self.indent.clone();
                self.insert_text(&indent, false)
            }
            KeyCode::BackTab => self.outdent(),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete_forward(),
            KeyCode::Left => {
                if let Some(p) = self.prev_pos(self.cursor) {
                    self.cursor = p;
                }
                self.want_col = None;
                self.group_open = false;
            }
            KeyCode::Right => {
                if let Some(p) = self.next_pos(self.cursor) {
                    self.cursor = p;
                }
                self.want_col = None;
                self.group_open = false;
            }
            KeyCode::Up => {
                if self.cursor.row > 0 {
                    self.move_to_row(self.cursor.row - 1);
                }
                self.group_open = false;
            }
            KeyCode::Down => {
                if self.cursor.row + 1 < self.lines.len() {
                    self.move_to_row(self.cursor.row + 1);
                }
                self.group_open = false;
            }
            KeyCode::PageUp => {
                self.top = self.top.saturating_sub(page);
                self.move_to_row(self.cursor.row.saturating_sub(page));
                self.group_open = false;
            }
            KeyCode::PageDown => {
                let max_top = self.lines.len().saturating_sub(1);
                self.top = (self.top + page).min(max_top);
                self.move_to_row(self.cursor.row + page);
                self.group_open = false;
            }
            KeyCode::Home => {
                // Smart home: first non-blank, then column 0.
                let line = &self.lines[self.cursor.row];
                let first = line.len() - line.trim_start().len();
                self.cursor.col = if self.cursor.col == first { 0 } else { first };
                self.want_col = None;
                self.group_open = false;
            }
            KeyCode::End => {
                self.cursor.col = self.lines[self.cursor.row].len();
                self.want_col = None;
                self.group_open = false;
            }
            _ => {
                return if had_message {
                    EditorAction::Redraw
                } else {
                    EditorAction::None
                };
            }
        }
        self.ensure_visible();
        EditorAction::Redraw
    }

    fn request_close(&mut self) -> EditorAction {
        if self.modified() {
            self.confirm = Some(Confirm::CloseUnsaved);
            EditorAction::Redraw
        } else {
            EditorAction::Close
        }
    }

    fn handle_confirm_key(&mut self, confirm: Confirm, key: KeyEvent) -> EditorAction {
        let ch = match key.code {
            KeyCode::Char(c) => Some(c.to_ascii_lowercase()),
            _ => None,
        };
        match confirm {
            Confirm::CloseUnsaved => match (key.code, ch) {
                (_, Some('s')) | (_, Some('y')) => self.try_save(false, true),
                (_, Some('d')) | (_, Some('n')) => EditorAction::Close,
                (KeyCode::Esc, _) | (_, Some('c')) => {
                    self.confirm = None;
                    EditorAction::Redraw
                }
                _ => EditorAction::None,
            },
            Confirm::OverwriteExternal => match (key.code, ch) {
                (_, Some('o')) | (_, Some('y')) => self.try_save(true, false),
                (_, Some('r')) => {
                    self.confirm = None;
                    if let Err(e) = self.reload() {
                        self.message = Some((format!("다시 불러오기 실패: {e}"), true));
                    }
                    EditorAction::Redraw
                }
                (KeyCode::Esc, _) | (_, Some('n')) | (_, Some('c')) => {
                    self.confirm = None;
                    self.message = Some(("저장을 취소했습니다".to_string(), false));
                    EditorAction::Redraw
                }
                _ => EditorAction::None,
            },
        }
    }

    /// Note an outside change noticed on focus regain (shown once in the status row).
    pub fn note_external_change(&mut self) {
        if self.changed_on_disk() && self.confirm.is_none() {
            self.message = Some((
                "디스크의 파일이 바뀌었습니다 — 저장하면 덮어쓸지 묻습니다".to_string(),
                true,
            ));
        }
    }

    /// The draw model for the presenter: only the visible rows are copied.
    pub fn view(&self) -> FileEditorView {
        let rows = self.viewport.1.max(1) as usize;
        let end = (self.top + rows).min(self.lines.len());
        FileEditorView {
            title: self.title.clone(),
            first_line: self.top,
            total_lines: self.lines.len(),
            visible: self.lines[self.top..end].to_vec(),
            left: self.left,
            cursor_row: self.cursor.row,
            cursor_display_col: display_col(&self.lines[self.cursor.row], self.cursor.col),
            modified: self.modified(),
            crlf: self.format.crlf,
            confirm: self.confirm,
            message: self.message.clone(),
        }
    }
}

/// Write `data` to `path` atomically: a temp file beside it, permissions copied, then rename.
fn write_atomic(path: &Path, data: &[u8]) -> io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::other("상위 폴더가 없습니다"))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = dir.join(format!(".{name}.hfe-{}.tmp", std::process::id()));
    let result = (|| {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(data)?;
        file.sync_all()?;
        if let Ok(meta) = fs::metadata(path) {
            fs::set_permissions(&tmp, meta.permissions())?;
        }
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// What the presenter needs to draw the editor.
#[derive(Debug, Clone)]
pub struct FileEditorView {
    pub title: String,
    pub first_line: usize,
    pub total_lines: usize,
    pub visible: Vec<String>,
    pub left: usize,
    pub cursor_row: usize,
    pub cursor_display_col: usize,
    pub modified: bool,
    pub crlf: bool,
    pub confirm: Option<Confirm>,
    pub message: Option<(String, bool)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }
    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }
    fn editor(text: &str) -> FileEditor {
        FileEditor::from_text(
            PathBuf::from("/tmp/x"),
            PathBuf::from("/tmp/x"),
            "x".into(),
            text,
            DiskStamp {
                len: 0,
                modified: None,
            },
        )
    }
    fn type_str(e: &mut FileEditor, s: &str) {
        for c in s.chars() {
            if c == '\n' {
                e.handle_key(key(KeyCode::Enter));
            } else {
                e.handle_key(key(KeyCode::Char(c)));
            }
        }
    }

    #[test]
    fn round_trips_formats() {
        for raw in [
            "",
            "\n",
            "a",
            "a\n",
            "a\nb\n",
            "a\r\nb\r\n",
            "\u{feff}x\n",
            "a\n\n",
        ] {
            let (lines, fmt) = parse_text(raw);
            assert_eq!(serialize(&lines, fmt), raw, "round trip of {raw:?}");
        }
    }

    #[test]
    fn typing_enter_and_backspace() {
        let mut e = editor("hello\n");
        e.handle_key(key(KeyCode::End));
        type_str(&mut e, " 세계\nnext");
        assert_eq!(e.text(), "hello 세계\nnext\n");
        for _ in 0..5 {
            e.handle_key(key(KeyCode::Backspace));
        }
        assert_eq!(e.text(), "hello 세계\n");
        assert!(e.modified());
    }

    #[test]
    fn undo_redo_groups_typing() {
        let mut e = editor("ab\n");
        e.handle_key(key(KeyCode::End));
        type_str(&mut e, "cde");
        assert_eq!(e.text(), "abcde\n");
        e.handle_key(ctrl('z'));
        assert_eq!(e.text(), "ab\n");
        assert!(!e.modified());
        e.handle_key(ctrl('y'));
        assert_eq!(e.text(), "abcde\n");
    }

    #[test]
    fn delete_joins_lines_and_undoes() {
        let mut e = editor("one\ntwo\n");
        e.handle_key(key(KeyCode::End));
        e.handle_key(key(KeyCode::Delete));
        assert_eq!(e.text(), "onetwo\n");
        e.handle_key(ctrl('z'));
        assert_eq!(e.text(), "one\ntwo\n");
    }

    #[test]
    fn paste_is_one_undo_step() {
        let mut e = editor("x\n");
        e.paste("a\r\nb\nc");
        assert_eq!(e.text(), "a\nb\ncx\n");
        e.handle_key(ctrl('z'));
        assert_eq!(e.text(), "x\n");
    }

    #[test]
    fn vertical_move_keeps_display_column_with_wide_chars() {
        let mut e = editor("가나다라\nabcdefgh\n");
        e.handle_key(key(KeyCode::Right));
        e.handle_key(key(KeyCode::Right));
        e.handle_key(key(KeyCode::Down));
        assert_eq!(e.cursor(), Pos::new(1, 4));
    }

    #[test]
    fn close_with_changes_asks_first() {
        let mut e = editor("a\n");
        type_str(&mut e, "b");
        assert_eq!(e.handle_key(key(KeyCode::Esc)), EditorAction::Redraw);
        assert_eq!(e.confirm(), Some(Confirm::CloseUnsaved));
        assert_eq!(e.handle_key(key(KeyCode::Esc)), EditorAction::Redraw);
        assert_eq!(e.confirm(), None);
        e.handle_key(key(KeyCode::Esc));
        assert_eq!(e.handle_key(key(KeyCode::Char('d'))), EditorAction::Close);
    }

    #[test]
    fn tab_uses_detected_indent_and_backtab_outdents() {
        let mut e = editor("fn x() {\n  a\n}\n");
        e.handle_key(key(KeyCode::Down));
        e.handle_key(key(KeyCode::Home));
        e.handle_key(key(KeyCode::Tab));
        assert_eq!(e.lines()[1], "    a");
        e.handle_key(key(KeyCode::BackTab));
        assert_eq!(e.lines()[1], "  a");
    }

    #[test]
    fn saves_atomically_and_detects_outside_change() {
        let dir = std::env::temp_dir().join(format!("hfe-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("note.txt");
        fs::write(&file, "one\r\n").unwrap();
        let mut e = FileEditor::open(&file, "note.txt".into()).unwrap();
        e.handle_key(key(KeyCode::End));
        type_str(&mut e, "!");
        assert_eq!(e.handle_key(ctrl('s')), EditorAction::Saved);
        assert_eq!(fs::read_to_string(&file).unwrap(), "one!\r\n");
        assert!(!e.modified());

        // Outside change → save asks before overwriting.
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(&file, "changed elsewhere, longer\n").unwrap();
        type_str(&mut e, "?");
        assert_eq!(e.handle_key(ctrl('s')), EditorAction::Redraw);
        assert_eq!(e.confirm(), Some(Confirm::OverwriteExternal));
        assert_eq!(e.handle_key(key(KeyCode::Char('o'))), EditorAction::Saved);
        assert_eq!(fs::read_to_string(&file).unwrap(), "one!?\r\n");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn refuses_binary_and_non_utf8() {
        let dir = std::env::temp_dir().join(format!("hfe-test-bin-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("b");
        fs::write(&bin, [0x41, 0x00, 0x42]).unwrap();
        assert_eq!(
            FileEditor::open(&bin, "b".into()).unwrap_err(),
            OpenError::Binary
        );
        let latin = dir.join("l");
        fs::write(&latin, [0xC0, 0x41]).unwrap();
        assert_eq!(
            FileEditor::open(&latin, "l".into()).unwrap_err(),
            OpenError::NotUtf8
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
