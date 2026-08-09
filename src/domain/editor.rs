//! A plain-text buffer for editing a document in place.
//!
//! The buffer is loaded from disk rather than from the parsed document: a
//! [`SpecDocument`](crate::domain::spec::SpecDocument) carries its body with the
//! frontmatter already stripped, and saving that back would silently delete the
//! block. What is edited here is the file, byte for byte.
//!
//! Text is held as `Vec<char>` per line. Editing addresses characters, and a
//! `String` would make every insertion in a line containing an accent an
//! O(n) byte-boundary search; a workspace written in Spanish hits that on the
//! first word.

use std::path::{Path, PathBuf};

use crate::domain::util::write_atomic;
use crate::error::{Error, Result};

/// Spaces written by the Tab key. Markdown nests on two.
pub const TAB_WIDTH: usize = 2;

/// Largest file the editor will open, in bytes.
///
/// The buffer holds the whole file in memory and rewrites it whole, so opening
/// something enormous is a way to lose it, not a way to edit it.
pub const MAX_EDITABLE_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct Buffer {
    pub path: PathBuf,
    lines: Vec<Vec<char>>,
    pub row: usize,
    pub column: usize,
    /// Column a vertical move aims for, so crossing a short line does not
    /// shorten the cursor for good.
    goal: usize,
    pub dirty: bool,
    /// Whether the file ended in a newline, which is restored on save.
    final_newline: bool,
}

impl Buffer {
    pub fn open(path: &Path) -> Result<Self> {
        let size = std::fs::metadata(path)
            .map_err(|source| Error::io(path, source))?
            .len();
        if size > MAX_EDITABLE_BYTES {
            return Err(Error::TooLargeToEdit(path.to_path_buf()));
        }

        let raw = std::fs::read_to_string(path).map_err(|source| Error::io(path, source))?;
        Ok(Self::from_text(path, &raw))
    }

    fn from_text(path: &Path, raw: &str) -> Self {
        let final_newline = raw.ends_with('\n');
        let mut lines: Vec<Vec<char>> = raw.lines().map(|line| line.chars().collect()).collect();
        // An empty file still has one line to put the cursor on.
        if lines.is_empty() {
            lines.push(Vec::new());
        }

        Self {
            path: path.to_path_buf(),
            lines,
            row: 0,
            column: 0,
            goal: 0,
            dirty: false,
            final_newline,
        }
    }

    // ---- Reading --------------------------------------------------------

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn line(&self, index: usize) -> String {
        self.lines
            .get(index)
            .map(|line| line.iter().collect())
            .unwrap_or_default()
    }

    pub fn line_len(&self, index: usize) -> usize {
        self.lines.get(index).map_or(0, Vec::len)
    }

    /// The whole buffer as it would be written.
    pub fn text(&self) -> String {
        let mut out = self
            .lines
            .iter()
            .map(|line| line.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        if self.final_newline {
            out.push('\n');
        }
        out
    }

    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_string()
    }

    // ---- Writing --------------------------------------------------------

    pub fn insert(&mut self, character: char) {
        let column = self.column.min(self.line_len(self.row));
        self.lines[self.row].insert(column, character);
        self.column = column + 1;
        self.goal = self.column;
        self.dirty = true;
    }

    pub fn indent(&mut self) {
        for _ in 0..TAB_WIDTH {
            self.insert(' ');
        }
    }

    pub fn newline(&mut self) {
        let column = self.column.min(self.line_len(self.row));
        let tail = self.lines[self.row].split_off(column);

        // Markdown is written in nested lists; carrying the indentation over is
        // what the next line almost always wants.
        let indent: Vec<char> = self.lines[self.row]
            .iter()
            .take_while(|character| **character == ' ')
            .copied()
            .collect();
        let carried = indent.len();

        let mut next = indent;
        next.extend(tail);

        self.lines.insert(self.row + 1, next);
        self.row += 1;
        self.column = carried;
        self.goal = self.column;
        self.dirty = true;
    }

    pub fn backspace(&mut self) {
        if self.column > 0 {
            let column = self.column.min(self.line_len(self.row));
            self.lines[self.row].remove(column - 1);
            self.column = column - 1;
        } else if self.row > 0 {
            let current = self.lines.remove(self.row);
            self.row -= 1;
            self.column = self.line_len(self.row);
            self.lines[self.row].extend(current);
        } else {
            return;
        }

        self.goal = self.column;
        self.dirty = true;
    }

    pub fn delete(&mut self) {
        let length = self.line_len(self.row);
        if self.column < length {
            self.lines[self.row].remove(self.column);
        } else if self.row + 1 < self.lines.len() {
            let next = self.lines.remove(self.row + 1);
            self.lines[self.row].extend(next);
        } else {
            return;
        }

        self.dirty = true;
    }

    /// Removes the line under the cursor.
    pub fn delete_line(&mut self) {
        if self.lines.len() == 1 {
            self.lines[0].clear();
        } else {
            self.lines.remove(self.row);
            self.row = self.row.min(self.lines.len() - 1);
        }
        self.column = 0;
        self.goal = 0;
        self.dirty = true;
    }

    pub fn save(&mut self) -> Result<()> {
        write_atomic(&self.path, &self.text())?;
        self.dirty = false;
        Ok(())
    }

    // ---- Moving ---------------------------------------------------------

    pub fn move_to(&mut self, row: usize, column: usize) {
        self.row = row.min(self.lines.len().saturating_sub(1));
        self.column = column.min(self.line_len(self.row));
        self.goal = self.column;
    }

    /// Moves one character left or right, wrapping across line ends.
    pub fn step_horizontal(&mut self, delta: isize) {
        if delta < 0 {
            if self.column > 0 {
                self.column -= 1;
            } else if self.row > 0 {
                self.row -= 1;
                self.column = self.line_len(self.row);
            }
        } else if self.column < self.line_len(self.row) {
            self.column += 1;
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.column = 0;
        }

        self.goal = self.column;
    }

    /// Moves by whole lines, keeping the column the cursor is aiming for.
    pub fn step_vertical(&mut self, delta: isize) {
        let last = self.lines.len().saturating_sub(1) as isize;
        let target = (self.row as isize + delta).clamp(0, last) as usize;
        self.row = target;
        self.column = self.goal.min(self.line_len(self.row));
    }

    pub fn home(&mut self) {
        // The first press lands on the text, the second on the margin: leading
        // spaces are structure in Markdown and jumping past them is the common
        // intent.
        let indent = self.lines[self.row]
            .iter()
            .take_while(|character| **character == ' ')
            .count();
        self.column = if self.column == indent { 0 } else { indent };
        self.goal = self.column;
    }

    pub fn end(&mut self) {
        self.column = self.line_len(self.row);
        self.goal = self.column;
    }

    pub fn first(&mut self) {
        self.move_to(0, 0);
    }

    pub fn last(&mut self) {
        let row = self.lines.len().saturating_sub(1);
        self.move_to(row, self.line_len(row));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer(text: &str) -> Buffer {
        Buffer::from_text(Path::new("/tmp/example.md"), text)
    }

    #[test]
    fn round_trips_a_file_without_touching_it() {
        let raw = "# Title\n\nBody text.\n";
        assert_eq!(buffer(raw).text(), raw);
        assert_eq!(buffer("no newline").text(), "no newline");
    }

    #[test]
    fn inserts_on_character_boundaries() {
        let mut buffer = buffer("especificación\n");
        buffer.end();
        buffer.insert('!');
        assert_eq!(buffer.text(), "especificación!\n");
        assert!(buffer.dirty);
    }

    #[test]
    fn a_new_line_carries_the_indentation() {
        let mut buffer = buffer("  - item\n");
        buffer.end();
        buffer.newline();
        buffer.insert('x');
        assert_eq!(buffer.text(), "  - item\n  x\n");
        assert_eq!(buffer.column, 3);
    }

    #[test]
    fn backspace_joins_two_lines() {
        let mut buffer = buffer("one\ntwo\n");
        buffer.move_to(1, 0);
        buffer.backspace();
        assert_eq!(buffer.text(), "onetwo\n");
        assert_eq!((buffer.row, buffer.column), (0, 3));
    }

    #[test]
    fn a_vertical_move_remembers_the_column() {
        let mut buffer = buffer("longest line\nshort\nlongest line\n");
        buffer.move_to(0, 11);
        buffer.step_vertical(1);
        assert_eq!(buffer.column, 5);
        buffer.step_vertical(1);
        assert_eq!(buffer.column, 11);
    }

    #[test]
    fn home_toggles_between_the_text_and_the_margin() {
        let mut buffer = buffer("    indented\n");
        buffer.end();
        buffer.home();
        assert_eq!(buffer.column, 4);
        buffer.home();
        assert_eq!(buffer.column, 0);
    }

    #[test]
    fn saving_writes_the_file_and_clears_the_flag() {
        let path = std::env::temp_dir().join(format!(
            "openspectation-editor-{}-{}.md",
            std::process::id(),
            line!()
        ));
        std::fs::write(&path, "one\n").unwrap();

        let mut buffer = Buffer::open(&path).unwrap();
        buffer.end();
        buffer.insert('!');
        buffer.save().unwrap();

        let raw = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(raw, "one!\n");
        assert!(!buffer.dirty);
    }

    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    /// A path in a unique temp file, removed when the guard drops.
    struct TempFile(PathBuf);

    impl TempFile {
        fn new(contents: &str) -> Self {
            let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "openspectation-editor-{}-{unique}.md",
                std::process::id()
            ));
            std::fs::write(&path, contents).unwrap();
            Self(path)
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn an_empty_file_still_has_a_line_to_put_the_cursor_on() {
        let buffer = buffer("");
        assert_eq!(buffer.len(), 1);
        assert_eq!(buffer.line(0), "");
        // `is_empty` exists to satisfy the `len` lint; the invariant above means
        // it is never true.
        assert!(!buffer.is_empty());
    }

    #[test]
    fn reading_past_the_last_line_yields_nothing_rather_than_panicking() {
        let buffer = buffer("one\n");
        assert_eq!(buffer.line(99), "");
        assert_eq!(buffer.line_len(99), 0);
    }

    #[test]
    fn the_file_name_comes_from_the_path() {
        assert_eq!(buffer("x").file_name(), "example.md");
        assert_eq!(
            Buffer::from_text(Path::new("/"), "x").file_name(),
            String::new()
        );
    }

    #[test]
    fn opening_a_missing_file_reports_the_path_that_failed() {
        let missing = std::env::temp_dir().join("openspectation-does-not-exist-xyz.md");
        match Buffer::open(&missing) {
            Err(Error::Io { path, .. }) => assert_eq!(path, missing),
            other => panic!("expected Io, got {other:?}"),
        }
    }

    #[test]
    fn a_file_over_the_ceiling_is_refused_before_it_is_read() {
        // Sparse, so the guard is exercised without writing four megabytes.
        let temp = TempFile::new("");
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(&temp.0)
            .unwrap();
        file.set_len(MAX_EDITABLE_BYTES + 1).unwrap();
        drop(file);

        assert!(matches!(
            Buffer::open(&temp.0),
            Err(Error::TooLargeToEdit(_))
        ));
    }

    #[test]
    fn a_file_at_the_ceiling_is_still_accepted() {
        let temp = TempFile::new("");
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(&temp.0)
            .unwrap();
        file.set_len(MAX_EDITABLE_BYTES).unwrap();
        drop(file);

        assert!(Buffer::open(&temp.0).is_ok());
    }

    #[test]
    fn saving_preserves_a_missing_trailing_newline() {
        let temp = TempFile::new("one");
        let mut buffer = Buffer::open(&temp.0).unwrap();
        buffer.end();
        buffer.insert('!');
        buffer.save().unwrap();
        assert_eq!(std::fs::read_to_string(&temp.0).unwrap(), "one!");
    }

    #[test]
    fn indent_writes_one_markdown_nesting_level() {
        let mut buffer = buffer("item\n");
        buffer.indent();
        assert_eq!(buffer.text(), "  item\n");
        assert_eq!(buffer.column, TAB_WIDTH);
    }

    #[test]
    fn a_newline_mid_line_moves_the_tail_down() {
        let mut buffer = buffer("onetwo\n");
        buffer.move_to(0, 3);
        buffer.newline();
        assert_eq!(buffer.text(), "one\ntwo\n");
        assert_eq!((buffer.row, buffer.column), (1, 0));
    }

    #[test]
    fn backspace_at_the_very_start_leaves_the_buffer_untouched() {
        let mut buffer = buffer("one\n");
        buffer.first();
        buffer.backspace();
        assert_eq!(buffer.text(), "one\n");
        assert!(!buffer.dirty);
    }

    #[test]
    fn delete_removes_forward_and_then_joins_the_next_line() {
        let mut buffer = buffer("one\ntwo\n");
        buffer.first();
        buffer.delete();
        assert_eq!(buffer.line(0), "ne");

        buffer.end();
        buffer.delete();
        assert_eq!(buffer.text(), "netwo\n");
    }

    #[test]
    fn delete_at_the_very_end_leaves_the_buffer_untouched() {
        let mut buffer = buffer("one\n");
        buffer.last();
        buffer.delete();
        assert_eq!(buffer.text(), "one\n");
        assert!(!buffer.dirty);
    }

    #[test]
    fn deleting_the_only_line_empties_it_instead_of_removing_it() {
        let mut buffer = buffer("one\n");
        buffer.delete_line();
        assert_eq!(buffer.len(), 1);
        assert_eq!(buffer.text(), "\n");
        assert!(buffer.dirty);
    }

    #[test]
    fn deleting_the_last_line_pulls_the_cursor_back() {
        let mut buffer = buffer("one\ntwo\n");
        buffer.move_to(1, 2);
        buffer.delete_line();
        assert_eq!(buffer.text(), "one\n");
        assert_eq!((buffer.row, buffer.column), (0, 0));
    }

    #[test]
    fn stepping_horizontally_wraps_across_line_ends_in_both_directions() {
        let mut buffer = buffer("ab\ncd\n");
        buffer.move_to(0, 2);
        buffer.step_horizontal(1);
        assert_eq!((buffer.row, buffer.column), (1, 0));
        buffer.step_horizontal(-1);
        assert_eq!((buffer.row, buffer.column), (0, 2));
    }

    #[test]
    fn stepping_horizontally_stops_at_both_ends_of_the_buffer() {
        let mut buffer = buffer("ab\n");
        buffer.first();
        buffer.step_horizontal(-1);
        assert_eq!((buffer.row, buffer.column), (0, 0));
        buffer.last();
        buffer.step_horizontal(1);
        assert_eq!((buffer.row, buffer.column), (0, 2));
    }

    #[test]
    fn a_vertical_move_is_clamped_to_the_document() {
        let mut buffer = buffer("one\ntwo\nthree\n");
        buffer.step_vertical(-10);
        assert_eq!(buffer.row, 0);
        buffer.step_vertical(10);
        assert_eq!(buffer.row, 2);
    }

    #[test]
    fn move_to_clamps_both_coordinates() {
        let mut buffer = buffer("one\ntwo\n");
        buffer.move_to(99, 99);
        assert_eq!((buffer.row, buffer.column), (1, 3));
    }

    #[test]
    fn first_and_last_jump_to_the_document_edges() {
        let mut buffer = buffer("one\ntwo three\n");
        buffer.last();
        assert_eq!((buffer.row, buffer.column), (1, 9));
        buffer.first();
        assert_eq!((buffer.row, buffer.column), (0, 0));
    }

    #[test]
    fn home_on_a_line_without_indentation_stays_at_the_margin() {
        let mut buffer = buffer("plain\n");
        buffer.end();
        buffer.home();
        assert_eq!(buffer.column, 0);
        buffer.home();
        assert_eq!(buffer.column, 0);
    }

    #[test]
    fn moving_never_marks_the_buffer_dirty() {
        // Only the write operations may set the flag; the editor uses it to
        // decide whether closing needs a confirmation.
        let mut buffer = buffer("one\ntwo\n");
        buffer.last();
        buffer.first();
        buffer.end();
        buffer.home();
        buffer.step_vertical(1);
        buffer.step_horizontal(-1);
        buffer.move_to(0, 1);
        assert!(!buffer.dirty);
    }

    #[test]
    fn accented_text_is_addressed_by_character_not_by_byte() {
        let mut buffer = buffer("ñandú\n");
        assert_eq!(buffer.line_len(0), 5);
        buffer.move_to(0, 1);
        buffer.delete();
        assert_eq!(buffer.line(0), "ñndú");
        buffer.end();
        buffer.backspace();
        assert_eq!(buffer.text(), "ñnd\n");
    }
}
