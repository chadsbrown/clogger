use logger_runtime::CondXSnapshot;
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::theme::Theme;

const MAX_GEOMAG_CHARS: usize = 8;

pub fn render(frame: &mut Frame, area: Rect, condx: &Option<CondXSnapshot>, theme: &Theme) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" CondX ")
        .style(Style::from(theme.box_border));

    let lines: Vec<Line> = match condx {
        None => vec![Line::from(Span::styled(
            " Loading…",
            Style::from(theme.box_label),
        ))],
        Some(s) => snapshot_lines(s, theme),
    };

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn snapshot_lines(s: &CondXSnapshot, theme: &Theme) -> Vec<Line<'static>> {
    let label = Style::from(theme.box_label);
    let value = Style::from(theme.box_value);

    fn num(v: Option<u32>) -> String {
        v.map(|n| n.to_string()).unwrap_or_else(|| "-".to_string())
    }

    fn or_dash(s: &str) -> String {
        if s.is_empty() { "-".to_string() } else { s.to_string() }
    }

    let truncate_gmag = {
        let g = or_dash(&s.geomag_field);
        if g.chars().count() <= MAX_GEOMAG_CHARS {
            g
        } else {
            let mut t: String = g.chars().take(MAX_GEOMAG_CHARS - 1).collect();
            t.push('…');
            t
        }
    };

    let rating = |band: &str, time: &str| -> String {
        s.conditions
            .iter()
            .find(|c| c.band == band && c.time == time)
            .map(|c| first_letter(&c.rating))
            .unwrap_or_else(|| "-".to_string())
    };

    let mut lines = Vec::with_capacity(10);
    lines.push(Line::from(vec![
        Span::styled(" SFI:", label),
        Span::styled(format!("{:>4}", num(s.sfi)), value),
        Span::styled("  SN:", label),
        Span::styled(format!("{:>3}", num(s.sunspots)), value),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  A: ", label),
        Span::styled(format!("{:>3}", num(s.a_index)), value),
        Span::styled("   K:", label),
        Span::styled(format!("{:>3}", num(s.k_index)), value),
    ]));
    lines.push(Line::from(vec![
        Span::styled(" X-ray:  ", label),
        Span::styled(or_dash(&s.xray), value),
    ]));
    lines.push(Line::from(vec![
        Span::styled(" Gmag: ", label),
        Span::styled(truncate_gmag, value),
    ]));
    lines.push(Line::from(vec![
        Span::styled(" S/N:   ", label),
        Span::styled(or_dash(&s.signal_noise), value),
    ]));
    for &band in &["80m-40m", "30m-20m", "17m-15m", "12m-10m"] {
        let day = rating(band, "day");
        let night = rating(band, "night");
        let short = band.replace('m', "");
        lines.push(Line::from(vec![
            Span::styled(format!(" {short:<8}"), label),
            Span::styled(format!("{day}/{night}"), value),
        ]));
    }
    lines.push(Line::from(vec![
        Span::styled(" Upd ", label),
        Span::styled(short_updated(&s.updated), value),
    ]));
    lines
}

/// First letter of the rating, so "Poor" → "P", "Good" → "G", "Fair" → "F".
/// "-" for empty/unknown values.
fn first_letter(rating: &str) -> String {
    rating
        .chars()
        .next()
        .map(|c| c.to_string())
        .unwrap_or_else(|| "-".to_string())
}

/// Extract the `HHMM` time-of-day from hamqsl's format
/// `"18 Apr 2026 2057 GMT"` and render as `"20:57z"`. Falls back to the
/// raw string when the format doesn't match — we'd rather show something
/// the operator can read than hide a mismatch silently.
fn short_updated(raw: &str) -> String {
    let parts: Vec<&str> = raw.split_whitespace().collect();
    // Expect: [day, month, year, HHMM, tz]
    if parts.len() >= 4 && parts[3].len() == 4 && parts[3].chars().all(|c| c.is_ascii_digit()) {
        let hh = &parts[3][..2];
        let mm = &parts[3][2..];
        return format!("{hh}:{mm}z");
    }
    raw.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_updated_parses_hamqsl_format() {
        assert_eq!(short_updated("18 Apr 2026 2057 GMT"), "20:57z");
        assert_eq!(short_updated("18 Apr 2026 0000 GMT"), "00:00z");
    }

    #[test]
    fn short_updated_falls_back_on_garbage() {
        assert_eq!(short_updated("weird"), "weird");
        assert_eq!(short_updated(""), "");
    }

    #[test]
    fn first_letter_picks_rating_initial() {
        assert_eq!(first_letter("Poor"), "P");
        assert_eq!(first_letter("Good"), "G");
        assert_eq!(first_letter("Fair"), "F");
        assert_eq!(first_letter(""), "-");
    }
}
