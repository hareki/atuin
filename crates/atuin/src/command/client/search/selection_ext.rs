use ratatui::style::{Color, Style};

use crate::command::client::theme::Theme;

/// Converts crossterm Color to ratatui Color
fn convert_color(color: crossterm::style::Color) -> Color {
    match color {
        crossterm::style::Color::Rgb { r, g, b } => Color::Rgb(r, g, b),
        crossterm::style::Color::AnsiValue(v) => Color::Indexed(v),
        crossterm::style::Color::Black => Color::Black,
        crossterm::style::Color::DarkGrey => Color::DarkGray,
        crossterm::style::Color::Red | crossterm::style::Color::DarkRed => Color::Red,
        crossterm::style::Color::Green | crossterm::style::Color::DarkGreen => Color::Green,
        crossterm::style::Color::Yellow | crossterm::style::Color::DarkYellow => Color::Yellow,
        crossterm::style::Color::Blue | crossterm::style::Color::DarkBlue => Color::Blue,
        crossterm::style::Color::Magenta | crossterm::style::Color::DarkMagenta => Color::Magenta,
        crossterm::style::Color::Cyan | crossterm::style::Color::DarkCyan => Color::Cyan,
        crossterm::style::Color::White => Color::White,
        crossterm::style::Color::Grey => Color::Gray,
        crossterm::style::Color::Reset => Color::Reset,
    }
}

/// Returns a ratatui Style with the selection background color from theme
pub fn get_selection_style(theme: &Theme) -> Style {
    let content_style = theme.get_selection();
    let mut style = Style::default();
    if let Some(bg) = content_style.background_color {
        style = style.bg(convert_color(bg));
    }
    style
}
