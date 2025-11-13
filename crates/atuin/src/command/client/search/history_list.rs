use std::time::Duration;

use super::duration::format_duration;
use super::engines::SearchEngine;
use atuin_client::{
    history::History,
    theme::{Meaning, Theme},
};
use atuin_common::utils::Escapable as _;
use itertools::Itertools;
use ratatui::{
    buffer::Buffer,
    crossterm::style::{self, Color as CrosstermColor},
    layout::Rect,
    style::{Color, Modifier, Style},
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
    /// Apply an alternative highlighting to the selected row
    alternate_highlight: bool,
    now: &'a dyn Fn() -> OffsetDateTime,
    indicator: &'a str,
    theme: &'a Theme,
    history_highlighter: HistoryHighlighter<'a>,
    show_numeric_shortcuts: bool,
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
            alternate_highlight: self.alternate_highlight,
            now: &self.now,
            indicator: self.indicator,
            theme: self.theme,
            history_highlighter: self.history_highlighter,
            show_numeric_shortcuts: self.show_numeric_shortcuts,
        };

        for item in self.history.iter().skip(state.offset).take(end - start) {
            s.draw(" ", Style::default());
            s.duration(item);
            s.time(item);
            s.command(item);
            s.fill_row_background();

            // reset line
            s.y += 1;
            s.x = 0;
        }
    }
}

impl<'a> HistoryList<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        history: &'a [History],
        inverted: bool,
        alternate_highlight: bool,
        now: &'a dyn Fn() -> OffsetDateTime,
        indicator: &'a str,
        theme: &'a Theme,
        history_highlighter: HistoryHighlighter<'a>,
        show_numeric_shortcuts: bool,
    ) -> Self {
        Self {
            history,
            block: None,
            inverted,
            alternate_highlight,
            now,
            indicator,
            theme,
            history_highlighter,
            show_numeric_shortcuts,
        }
    }

    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    fn get_items_bounds(&self, selected: usize, offset: usize, height: usize) -> (usize, usize) {
        let offset = offset.min(self.history.len().saturating_sub(1));

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
    alternate_highlight: bool,
    now: &'a dyn Fn() -> OffsetDateTime,
    #[allow(dead_code)]
    indicator: &'a str,
    theme: &'a Theme,
    history_highlighter: HistoryHighlighter<'a>,
    #[allow(dead_code)]
    show_numeric_shortcuts: bool,
}

// longest line prefix I could come up with
#[allow(clippy::cast_possible_truncation)] // we know that this is <65536 length
pub const PREFIX_LENGTH: u16 = " 123ms 59s ago".len() as u16;
static SPACES: &str = "              ";
static _ASSERT: () = assert!(SPACES.len() == PREFIX_LENGTH as usize);

impl DrawState<'_> {
    fn duration(&mut self, h: &History) {
        let status = self.theme.as_style(if h.success() {
            Meaning::AlertInfo
        } else {
            Meaning::AlertError
        });
        let duration = Duration::from_nanos(u64::try_from(h.duration).unwrap_or(0));
        self.draw(&format_duration(duration), status.into());
    }

    #[allow(clippy::cast_possible_truncation)] // we know that time.len() will be <6
    fn time(&mut self, h: &History) {
        let mut style = self.theme.as_style(Meaning::Guidance);
        let is_selected = !self.alternate_highlight
            && (self.y as usize + self.state.offset == self.state.selected());
        if is_selected {
            style.background_color = Some(CrosstermColor::Rgb {
                r: 0x31,
                g: 0x32,
                b: 0x44,
            });
        }

        // Account for the chance that h.timestamp is "in the future"
        // This would mean that "since" is negative, and the unwrap here
        // would fail.
        // If the timestamp would otherwise be in the future, display
        // the time since as 0.
        let since = (self.now)() - h.timestamp;
        let time = format_duration(since.try_into().unwrap_or_default());

        // pad the time a little bit before we write. this aligns things nicely
        // skip padding if for some reason it is already too long to align nicely
        let padding =
            usize::from(PREFIX_LENGTH).saturating_sub(usize::from(self.x) + 4 + time.len());
        let mut padding_style = Style::default();
        if is_selected {
            padding_style = padding_style.bg(Color::Rgb(0x31, 0x32, 0x44));
        }
        self.draw(&SPACES[..padding], padding_style);

        self.draw(&time, style.into());
        self.draw(" ago", style.into());
    }

    fn command(&mut self, h: &History) {
        let style = self.theme.as_style(Meaning::Base);
        let mut row_highlighted = false;
        if !self.alternate_highlight
            && (self.y as usize + self.state.offset == self.state.selected())
        {
            row_highlighted = true;
        }

        let highlight_indices = self.history_highlighter.get_highlight_indices(
            h.command
                .escape_control()
                .split_ascii_whitespace()
                .join(" ")
                .as_str(),
        );

        let mut pos = 0;
        for section in h.command.escape_control().split_ascii_whitespace() {
            self.draw(" ", style.into());
            for ch in section.chars() {
                if self.x > self.list_area.width {
                    // Avoid attempting to draw a command section beyond the width
                    // of the list
                    return;
                }
                let mut style = style;
                if highlight_indices.contains(&pos) {
                    if row_highlighted {
                        // if the row is highlighted bold is not enough as the whole row is bold
                        // change the color too
                        style = self.theme.as_style(Meaning::AlertWarn);
                    }
                    style.attributes.set(style::Attribute::Bold);
                }
                self.draw(&ch.to_string(), style.into());
                pos += 1;
            }
            pos += 1;
        }
    }

    fn fill_row_background(&mut self) {
        if !self.alternate_highlight
            && (self.y as usize + self.state.offset == self.state.selected())
        {
            // Fill the rest of the row with the background color
            let remaining = (self.list_area.width.saturating_sub(self.x)) as usize;
            if remaining > 0 {
                if let Some(bg) = self.theme.as_style(Meaning::Selection).background_color {
                    let ratatui_color = match bg {
                        CrosstermColor::Rgb { r, g, b } => Color::Rgb(r, g, b),
                        _ => Color::Rgb(0x31, 0x32, 0x44), // fallback
                    };
                    let style = Style::default().bg(ratatui_color);
                    self.draw(&" ".repeat(remaining), style);
                }
            }
        }
    }

    fn draw(&mut self, s: &str, mut style: Style) {
        let cx = self.list_area.left() + self.x;

        let cy = if self.inverted {
            self.list_area.top() + self.y
        } else {
            self.list_area.bottom() - self.y - 1
        };

        // Apply background for selected row (non-alternate highlight mode)
        if !self.alternate_highlight
            && (self.y as usize + self.state.offset == self.state.selected())
        {
            if let Some(bg) = self.theme.as_style(Meaning::Selection).background_color {
                let ratatui_color = match bg {
                    CrosstermColor::Rgb { r, g, b } => Color::Rgb(r, g, b),
                    _ => Color::Rgb(0x31, 0x32, 0x44), // fallback
                };
                style = style.bg(ratatui_color);
            }
        }

        if self.alternate_highlight
            && (self.y as usize + self.state.offset == self.state.selected())
        {
            style = style.add_modifier(Modifier::REVERSED);
        }

        let w = (self.list_area.width - self.x) as usize;
        self.x += self.buf.set_stringn(cx, cy, s, w, style).0 - cx;
    }
}
