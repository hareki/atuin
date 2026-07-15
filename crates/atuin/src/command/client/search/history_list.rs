use std::time::Duration;

use super::duration::format_duration;
use super::engines::SearchEngine;
use super::selection_ext::get_selection_style;
use atuin_client::{
    history::History,
    settings::{UiColumn, UiColumnType},
    theme::{Meaning, Theme},
};
use atuin_common::string::EscapeNonPrintablePosixExt as _;
use itertools::Itertools;
use ratatui::{
    backend::FromCrossterm,
    buffer::Buffer,
    layout::Rect,
    style::Style,
    widgets::{Block, StatefulWidget, Widget},
};
use time::OffsetDateTime;

pub struct HistoryHighlighter<'a> {
    pub engine: &'a dyn SearchEngine,
    pub search_input: &'a str,
}

impl HistoryHighlighter<'_> {
    pub fn get_highlight_indices(&self, command: &str) -> Vec<usize> {
        self.engine
            .get_highlight_indices(command, self.search_input)
    }
}

pub struct HistoryList<'a> {
    history: &'a [History],
    block: Option<Block<'a>>,
    inverted: bool,
    now: &'a dyn Fn() -> OffsetDateTime,
    theme: &'a Theme,
    history_highlighter: HistoryHighlighter<'a>,
    /// Columns to display (in order, after the left padding)
    columns: &'a [UiColumn],
}

#[derive(Default)]
pub struct ListState {
    offset: usize,
    selected: usize,
    max_entries: usize,
}

impl ListState {
    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn max_entries(&self) -> usize {
        self.max_entries
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn select(&mut self, index: usize) {
        self.selected = index;
    }
}

impl StatefulWidget for HistoryList<'_> {
    type State = ListState;

    fn render(mut self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let list_area = self.block.take().map_or(area, |b| {
            let inner_area = b.inner(area);
            b.render(area, buf);
            inner_area
        });

        if list_area.width < 1 || list_area.height < 1 || self.history.is_empty() {
            return;
        }
        let list_height = list_area.height as usize;

        let (start, end) = self.get_items_bounds(state.selected, state.offset, list_height);
        state.offset = start;
        state.max_entries = end - start;

        let mut s = DrawState {
            buf,
            list_area,
            x: 0,
            y: 0,
            state,
            inverted: self.inverted,
            now: &self.now,
            theme: self.theme,
            history_highlighter: self.history_highlighter,
            columns: self.columns,
        };

        for item in self.history.iter().skip(state.offset).take(end - start) {
            s.render_row(item);

            // reset line
            s.y += 1;
            s.x = 0;
        }
    }
}

impl<'a> HistoryList<'a> {
    pub fn new(
        history: &'a [History],
        inverted: bool,
        now: &'a dyn Fn() -> OffsetDateTime,
        theme: &'a Theme,
        history_highlighter: HistoryHighlighter<'a>,
        columns: &'a [UiColumn],
    ) -> Self {
        Self {
            history,
            block: None,
            inverted,
            now,
            theme,
            history_highlighter,
            columns,
        }
    }

    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    fn get_items_bounds(&self, selected: usize, offset: usize, height: usize) -> (usize, usize) {
        let offset = offset.min(self.history.len().saturating_sub(1));

        // let max_scroll_space = height.min(10).min(self.history.len() - selected);
        let scroll_margin = height / 2;
        let max_scroll_space = height
            .saturating_sub(scroll_margin)
            .min(self.history.len() - selected);
        if offset + height < selected + max_scroll_space {
            let end = selected + max_scroll_space;
            (end - height, end)
        } else if selected < offset {
            (selected, selected + height)
        } else {
            (offset, offset + height)
        }
    }
}

struct DrawState<'a> {
    buf: &'a mut Buffer,
    list_area: Rect,
    x: u16,
    y: u16,
    state: &'a ListState,
    inverted: bool,
    now: &'a dyn Fn() -> OffsetDateTime,
    theme: &'a Theme,
    history_highlighter: HistoryHighlighter<'a>,
    columns: &'a [UiColumn],
}

impl DrawState<'_> {
    /// Check if current row is selected
    fn is_selected(&self) -> bool {
        self.y as usize + self.state.offset == self.state.selected()
    }

    /// Render a complete row for a history item based on configured columns.
    fn render_row(&mut self, h: &History) {
        // Draw left padding (1 space)
        self.left_padding();

        // Calculate the width for the expanding column
        // Fixed columns use their configured width + 1 (trailing space)
        let padding_width: u16 = 1;
        let fixed_width: u16 = self
            .columns
            .iter()
            .filter(|c| !c.expand)
            .map(|c| c.width + 1)
            .sum();
        let expand_width = self
            .list_area
            .width
            .saturating_sub(padding_width + fixed_width);

        let style = self.theme.as_style(Meaning::Base);
        // Render each configured column
        for (idx, column) in self.columns.iter().enumerate() {
            if idx != 0 {
                self.draw(" ", Style::from_crossterm(style));
            }
            let width = if column.expand {
                expand_width
            } else {
                column.width
            };
            match column.column_type {
                UiColumnType::Duration => self.duration(h, width),
                UiColumnType::Time => self.time(h, width),
                UiColumnType::Datetime => self.datetime(h, width),
                UiColumnType::Directory => self.directory(h, width),
                UiColumnType::Host => self.host(h, width),
                UiColumnType::User => self.user(h, width),
                UiColumnType::Exit => self.exit_code(h, width),
                UiColumnType::Command => self.command(h, width),
            }
        }

        // Fill remaining row width with selection background if selected
        self.fill_row_remainder();
    }

    /// Draw 1 space left padding
    fn left_padding(&mut self) {
        self.draw(" ", Style::default());
    }

    /// Fill remaining row width with selection background (for selected rows)
    fn fill_row_remainder(&mut self) {
        if !self.is_selected() {
            return;
        }

        let selection_style = get_selection_style(self.theme);
        let remaining = self.list_area.width.saturating_sub(self.x);
        if remaining > 0 {
            let spaces = " ".repeat(remaining as usize);
            self.draw(&spaces, selection_style);
        }
    }

    fn duration(&mut self, h: &History, width: u16) {
        let style = self.theme.as_style(if h.success() {
            Meaning::AlertInfo
        } else {
            Meaning::AlertError
        });
        let duration = Duration::from_nanos(u64::try_from(h.duration).unwrap_or(0));
        let formatted = format_duration(duration);
        let w = width as usize;
        // Right-align duration within its column width, plus trailing space
        let display = format!("{formatted:>w$}");
        self.draw(&display, Style::from_crossterm(style));
    }

    fn time(&mut self, h: &History, width: u16) {
        let style = self.theme.as_style(Meaning::Guidance);

        // Account for the chance that h.timestamp is "in the future"
        // This would mean that "since" is negative, and the unwrap here
        // would fail.
        // If the timestamp would otherwise be in the future, display
        // the time since as 0.
        let since = (self.now)() - h.timestamp;
        let time = format_duration(since.try_into().unwrap_or_default());

        // Format as "Xs ago" right-aligned within column width
        let w = width as usize;
        let time_str = format!("{time} ago");

        let display = format!("{time_str:>w$}");
        self.draw(&display, Style::from_crossterm(style));
    }

    fn command(&mut self, h: &History, _width: u16) {
        let style = self.theme.as_style(Meaning::Base);

        // Build the normalized command string (whitespace-collapsed, control chars escaped)
        let normalized: String = h
            .command
            .escape_non_printable()
            .split_ascii_whitespace()
            .join(" ");

        let highlight_indices = self.history_highlighter.get_highlight_indices(&normalized);

        // Calculate the available width for the command text.
        // `self.x` is already past the indicator and any preceding columns,
        // so the remaining width is how far we can draw.
        let avail = (self.list_area.width.saturating_sub(self.x)) as usize;

        // Truncate long commands from the middle to show both start and end,
        // so users can identify commands even in narrow terminals (issue #3596).
        let (display, display_to_original) = truncate_middle(&normalized, avail);
        let normalized_byte_len = normalized.len();

        // Render each character of the display string, applying highlights
        // where the original position matches a highlight index.
        for (ch, &original_byte_pos) in display.chars().zip(display_to_original.iter()) {
            if self.x > self.list_area.width {
                return;
            }

            let char_style = if original_byte_pos < normalized_byte_len
                && highlight_indices.contains(&original_byte_pos)
            {
                self.theme.as_style(Meaning::Highlight)
            } else {
                style
            };

            let s = ch.to_string();
            self.draw(&s, Style::from_crossterm(char_style));
        }
    }

    /// Render the absolute datetime column (e.g., "2025-01-22 14:35")
    fn datetime(&mut self, h: &History, width: u16) {
        let style = self.theme.as_style(Meaning::Annotation);
        // Format: YYYY-MM-DD HH:MM
        let formatted = h
            .timestamp
            .format(
                &time::format_description::parse_borrowed::<1>(
                    "[year]-[month]-[day] [hour]:[minute]",
                )
                .expect("valid format"),
            )
            .unwrap_or_else(|_| "????-??-?? ??:??".to_string());
        let w = width as usize;
        let display = format!("{formatted:w$}");
        self.draw(&display, Style::from_crossterm(style));
    }

    /// Render the directory column (working directory, truncated)
    fn directory(&mut self, h: &History, width: u16) {
        let style = self.theme.as_style(Meaning::Annotation);
        let w = width as usize;
        let cwd = &h.cwd;
        let char_count = cwd.chars().count();
        // Truncate from the left with "..." if too long, plus trailing space
        // Use character count for comparison and skip for UTF-8 safety
        let display = if char_count > w && w >= 4 {
            let truncated: String = cwd.chars().skip(char_count - (w - 3)).collect();
            format!("...{truncated}")
        } else {
            format!("{cwd:w$}")
        };
        self.draw(&display, Style::from_crossterm(style));
    }

    /// Render the host column (just the hostname)
    fn host(&mut self, h: &History, width: u16) {
        let style = self.theme.as_style(Meaning::Annotation);
        let w = width as usize;
        // Database stores hostname as "hostname:username"
        let host = h.hostname.split(':').next().unwrap_or(&h.hostname);
        let char_count = host.chars().count();
        // Use character count for comparison and take for UTF-8 safety
        let display = if char_count > w && w >= 4 {
            let truncated: String = host.chars().take(w.saturating_sub(4)).collect();
            format!("{truncated}...")
        } else {
            format!("{host:w$}")
        };
        self.draw(&display, Style::from_crossterm(style));
    }

    /// Render the user column
    fn user(&mut self, h: &History, width: u16) {
        let style = self.theme.as_style(Meaning::Annotation);
        let w = width as usize;
        // Database stores hostname as "hostname:username"
        let user = h.hostname.split(':').nth(1).unwrap_or("");
        let char_count = user.chars().count();
        // Use character count for comparison and take for UTF-8 safety
        let display = if char_count > w && w >= 4 {
            let truncated: String = user.chars().take(w.saturating_sub(4)).collect();
            format!("{truncated}...")
        } else {
            format!("{user:w$}")
        };
        self.draw(&display, Style::from_crossterm(style));
    }

    /// Render the exit code column
    fn exit_code(&mut self, h: &History, width: u16) {
        let style = if h.success() {
            self.theme.as_style(Meaning::AlertInfo)
        } else {
            self.theme.as_style(Meaning::AlertError)
        };
        let w = width as usize;
        let display = format!("{:>w$}", h.exit);
        self.draw(&display, Style::from_crossterm(style));
    }

    fn draw(&mut self, s: &str, mut style: Style) {
        let cx = self.list_area.left() + self.x;

        let cy = if self.inverted {
            self.list_area.top() + self.y
        } else {
            self.list_area.bottom() - self.y - 1
        };

        // Apply selection background color to selected row
        if self.is_selected() {
            let selection_style = get_selection_style(self.theme);
            if let Some(bg) = selection_style.bg {
                style = style.bg(bg);
            }
        }

        let w = (self.list_area.width - self.x) as usize;
        self.x += self.buf.set_stringn(cx, cy, s, w, style).0 - cx;
    }
}

/// Truncate a string from the middle to fit within `avail` characters,
/// showing the start and end separated by "...".
///
/// Returns the truncated display string and a mapping from each display
/// character's byte position to the corresponding byte position in the
/// original string. Characters in the "..." sentinel map to
/// `original.len()` (an out-of-range value that will never match any
/// real highlight index).
///
/// If the string fits within `avail`, or `avail < 7` (too small to
/// truncate meaningfully), the original string is returned as-is with
/// a 1:1 mapping.
fn truncate_middle(original: &str, avail: usize) -> (String, Vec<usize>) {
    let char_byte_offsets: Vec<usize> = original.char_indices().map(|(bp, _)| bp).collect();
    let char_count = char_byte_offsets.len();

    if char_count <= avail || avail < 7 {
        return (original.to_string(), char_byte_offsets);
    }

    let remaining = avail - 3;
    let prefix_len = remaining.div_ceil(2);
    let suffix_len = remaining / 2;

    let prefix: String = original.chars().take(prefix_len).collect();
    let suffix: String = original.chars().skip(char_count - suffix_len).collect();
    let display = format!("{prefix}...{suffix}");

    let original_byte_len = original.len();
    let mut mapping = Vec::with_capacity(avail);
    mapping.extend_from_slice(&char_byte_offsets[..prefix_len]);
    mapping.extend(std::iter::repeat_n(original_byte_len, 3));
    for i in 0..suffix_len {
        mapping.push(char_byte_offsets[char_count - suffix_len + i]);
    }

    (display, mapping)
}

#[cfg(test)]
mod tests {
    use super::truncate_middle;

    #[test]
    fn test_no_truncation_when_fits() {
        let (display, mapping) = truncate_middle("hello", 10);
        assert_eq!(display, "hello");
        assert_eq!(mapping.len(), 5);
        // Mapping should be 1:1 with byte offsets
        assert_eq!(mapping, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn test_no_truncation_exact_fit() {
        let (display, _) = truncate_middle("hello", 5);
        assert_eq!(display, "hello");
    }

    #[test]
    fn test_no_truncation_when_too_small() {
        // avail < 7 — too small to truncate, return as-is
        let (display, _) = truncate_middle("hello world this is long", 6);
        assert_eq!(display, "hello world this is long");
    }

    #[test]
    fn test_truncation_shows_start_and_end() {
        let (display, _) = truncate_middle("abcdefghijklmnopqrstuvwxyz", 11);
        // avail=11, remaining=8, prefix=4, suffix=4
        // "abcd...wxyz"
        assert_eq!(display, "abcd...wxyz");
        assert_eq!(display.len(), 11);
    }

    #[test]
    fn test_truncation_preserves_byte_mapping() {
        let original = "abcdefghij";
        let (display, mapping) = truncate_middle(original, 7);
        // avail=7, remaining=4, prefix=2, suffix=2
        // "ab...ij"
        assert_eq!(display, "ab...ij");

        // Prefix chars map to original positions 0, 1
        assert_eq!(mapping[0], 0); // 'a'
        assert_eq!(mapping[1], 1); // 'b'
        // "..." maps to original.len() (out of range)
        assert_eq!(mapping[2], original.len());
        assert_eq!(mapping[3], original.len());
        assert_eq!(mapping[4], original.len());
        // Suffix chars map to original positions 8, 9
        assert_eq!(mapping[5], 8); // 'i'
        assert_eq!(mapping[6], 9); // 'j'
    }

    #[test]
    fn test_truncation_with_unicode() {
        let original = "héllo wörld thïs ïs ä löng cömmänd";
        let (display, mapping) = truncate_middle(original, 15);
        // Should still be 15 chars
        assert_eq!(display.chars().count(), 15);
        // Should contain "..."
        assert!(display.contains("..."));
        // Mapping length matches display char count
        assert_eq!(mapping.len(), 15);
    }

    #[test]
    fn test_truncation_minimum_width() {
        // avail=7 is the minimum for truncation
        let original = "abcdefghij";
        let (display, _) = truncate_middle(original, 7);
        assert_eq!(display, "ab...ij");
    }

    #[test]
    fn test_truncation_odd_remaining() {
        // avail=10, remaining=7, prefix=4 (ceil), suffix=3
        let original = "abcdefghijklmno"; // 15 chars
        let (display, _) = truncate_middle(original, 10);
        // "abcd...mno"
        assert_eq!(display, "abcd...mno");
        assert_eq!(display.len(), 10);
    }
}
