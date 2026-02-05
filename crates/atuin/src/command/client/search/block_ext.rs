use ratatui::{
    backend::FromCrossterm,
    layout::Alignment,
    style::Style,
    text::Line,
    widgets::{Block, BorderType},
};

use crate::command::client::theme::Theme;

/// Creates a Block with themed border styling (rounded, with theme-derived color).
/// Also sets `title_style` to match, since titles are used as inner separator lines.
pub fn themed_block(theme: &Theme) -> Block<'static> {
    let border_style = Style::from_crossterm(theme.get_border());
    Block::default()
        .border_style(border_style)
        .title_style(border_style)
        .border_type(BorderType::Rounded)
}

/// Creates a themed block with " Atuin " centered title for the main TUI container.
pub fn titled_block(theme: &Theme) -> Block<'static> {
    themed_block(theme)
        .title(Line::from(" Atuin "))
        .title_alignment(Alignment::Center)
}
