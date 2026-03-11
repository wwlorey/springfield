use crate::style;

const MIN_WIDTH: usize = 40;

pub fn render_box(title: &str, lines: &[String]) -> String {
    render_box_styled(title, lines, style::bold)
}

pub fn render_box_styled(
    title: &str,
    lines: &[String],
    title_style: impl Fn(&str) -> String,
) -> String {
    let title_len = title.chars().count();
    let content_width = lines
        .iter()
        .map(|l| l.chars().count() + 4)
        .max()
        .unwrap_or(0);
    let inner = content_width.max(title_len + 4).max(MIN_WIDTH);

    let mut out = String::new();

    let fill_len = inner.saturating_sub(title_len + 3);
    out.push_str(&style::dim("╭─ "));
    out.push_str(&title_style(title));
    out.push_str(&style::dim(&format!(" {}╮", "─".repeat(fill_len))));

    for line in lines {
        let pad = inner - 3 - line.chars().count();
        out.push('\n');
        out.push_str(&style::dim("│"));
        out.push_str(&format!("  {}{} ", line, " ".repeat(pad)));
        out.push_str(&style::dim("│"));
    }

    out.push('\n');
    out.push_str(&style::dim(&format!("╰{}╯", "─".repeat(inner))));

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip_ansi(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                // Skip until 'm'
                for c2 in chars.by_ref() {
                    if c2 == 'm' {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    #[test]
    fn title_only_box() {
        let result = strip_ansi(&render_box("Hello", &[]));
        let lines: Vec<&str> = result.split('\n').collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("Hello"));
        assert!(lines[0].starts_with("╭─ "));
        assert!(lines[0].ends_with('╮'));
        assert!(lines[1].starts_with('╰'));
        assert!(lines[1].ends_with('╯'));
    }

    #[test]
    fn box_with_body_lines() {
        let body = vec!["Mode:    AFK".to_string(), "Agent:   claude".to_string()];
        let result = strip_ansi(&render_box("Starting", &body));
        let lines: Vec<&str> = result.split('\n').collect();
        assert_eq!(lines.len(), 4);
        assert!(lines[1].starts_with('│'));
        assert!(lines[1].ends_with('│'));
        assert!(lines[2].starts_with('│'));
        assert!(lines[2].ends_with('│'));
    }

    #[test]
    fn right_borders_align() {
        let body = vec![
            "short".to_string(),
            "a much longer content line here".to_string(),
            "mid".to_string(),
        ];
        let result = strip_ansi(&render_box("Test", &body));
        let lines: Vec<&str> = result.split('\n').collect();
        let widths: Vec<usize> = lines.iter().map(|l| l.chars().count()).collect();
        assert!(
            widths.windows(2).all(|w| w[0] == w[1]),
            "widths not aligned: {:?}\n{}",
            widths,
            result
        );
    }

    #[test]
    fn min_width_enforced() {
        let result = strip_ansi(&render_box("Hi", &[]));
        let lines: Vec<&str> = result.split('\n').collect();
        let bottom_width = lines[1].chars().count();
        assert!(
            bottom_width >= MIN_WIDTH + 2,
            "bottom width {} less than min {}",
            bottom_width,
            MIN_WIDTH + 2
        );
    }

    #[test]
    fn long_title_expands_box() {
        let long_title = "This is a very long title that exceeds the minimum width significantly";
        let result = strip_ansi(&render_box(long_title, &[]));
        let lines: Vec<&str> = result.split('\n').collect();
        assert!(lines[0].contains(long_title));
        let widths: Vec<usize> = lines.iter().map(|l| l.chars().count()).collect();
        assert_eq!(
            widths[0], widths[1],
            "top/bottom mismatch: {:?}\n{}",
            widths, result
        );
    }

    #[test]
    fn long_content_expands_box() {
        let body = vec!["x".repeat(60)];
        let result = strip_ansi(&render_box("T", &body));
        let lines: Vec<&str> = result.split('\n').collect();
        let widths: Vec<usize> = lines.iter().map(|l| l.chars().count()).collect();
        assert!(
            widths.windows(2).all(|w| w[0] == w[1]),
            "widths not aligned: {:?}\n{}",
            widths,
            result
        );
    }
}
