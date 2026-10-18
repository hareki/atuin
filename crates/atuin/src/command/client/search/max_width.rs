use atuin_client::settings::Settings;
use ratatui::layout::Rect;

/// Narrows `area` so that the search UI is at most `max_width` columns wide,
/// keeping it at the left edge. A `max_width` of 0 means no limit.
pub fn clamp(area: Rect, settings: &Settings) -> Rect {
    if settings.max_width == 0 {
        return area;
    }
    Rect {
        // The layout keeps a 1-column margin on each side of the UI
        width: area.width.min(settings.max_width.saturating_add(2)),
        ..area
    }
}
