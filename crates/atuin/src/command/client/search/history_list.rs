use std::time::Duration;

use super::duration::format_duration;
use super::engines::SearchEngine;
use super::selection_ext::get_selection_style;
use atuin_client::{
    history::History,
    settings::{UiColumn, UiColumnType},
    theme::{Meaning, Theme},
};
use atuin_common::string::EllipsizeExt as _;
use atuin_common::string::EscapeNonPrintablePosixExt as _;
use atuin_common::string::ellipsis::{Budget, Indicator, Pos};
use itertools::Itertools;
use ratatui::{
    backend::FromCrossterm,
    buffer::Buffer,
    layout::Rect,
    style::Style,
    widgets::{Block, StatefulWidget, Widget},
};
use time::OffsetDateTime;
use unicode_width::UnicodeWidthStr;

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
        let ellipsized =
            normalized.ellipsize(Budget::Columns(avail), Pos::Middle, Indicator::UNICODE);
        let display = ellipsized.to_string();
        for (i, ch) in display.char_indices() {
            if self.x > self.list_area.width {
                return;
            }
            // Map each output cell back to its source byte and test the existing
            // highlight set; a cell on the spliced ellipsis maps to None and is
            // never highlighted (this is why the "…" never gets the highlight style).
            let highlighted = ellipsized
                .source_index(i)
                .is_some_and(|b| highlight_indices.contains(&b));
            let char_style = if highlighted {
                self.theme.as_style(Meaning::Highlight)
            } else {
                style
            };
            self.draw(&ch.to_string(), Style::from_crossterm(char_style));
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
        // Elide from the left with "..." so the leaf directory stays visible;
        // pad to the column width when it already fits.
        let display = if cwd.width() > w {
            cwd.ellipsize(Budget::Columns(w), Pos::Start, Indicator::UNICODE)
                .to_string()
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
        let display = if host.width() > w {
            host.ellipsize(Budget::Columns(w), Pos::End, Indicator::UNICODE)
                .to_string()
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
        let display = if user.width() > w {
            user.ellipsize(Budget::Columns(w), Pos::End, Indicator::UNICODE)
                .to_string()
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
