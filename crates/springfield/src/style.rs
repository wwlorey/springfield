use std::sync::OnceLock;

static NO_COLOR: OnceLock<bool> = OnceLock::new();

pub fn no_color() -> bool {
    *NO_COLOR.get_or_init(|| std::env::var("NO_COLOR").is_ok())
}

fn wrap(code: &str, s: &str, disabled: bool) -> String {
    if disabled {
        s.to_string()
    } else {
        format!("\x1b[{code}m{s}\x1b[0m")
    }
}

pub fn bold(s: &str) -> String {
    wrap("1", s, no_color())
}

pub fn dim(s: &str) -> String {
    wrap("2", s, no_color())
}

pub fn green(s: &str) -> String {
    wrap("32", s, no_color())
}

pub fn yellow(s: &str) -> String {
    wrap("33", s, no_color())
}

pub fn red(s: &str) -> String {
    wrap("31", s, no_color())
}

pub fn white(s: &str) -> String {
    wrap("37", s, no_color())
}

pub fn strip_ansi(s: &str) -> String {
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut out = Vec::with_capacity(len);
    let mut i = 0;
    while i < len {
        if bytes[i] == 0x1b && i + 1 < len && bytes[i + 1] == b'[' {
            i += 2;
            while i < len && (bytes[i].is_ascii_digit() || bytes[i] == b';') {
                i += 1;
            }
            if i < len && bytes[i] == b'm' {
                i += 1;
            }
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    unsafe { String::from_utf8_unchecked(out) }
}

pub fn badge_top() -> String {
    if no_color() {
        String::new()
    } else {
        dim("╭─────╮")
    }
}

pub fn badge_mid() -> String {
    if no_color() {
        "sgf:".to_string()
    } else {
        format!("{}\x1b[1;7m sgf \x1b[0m{}", dim("│"), dim("│"))
    }
}

pub fn badge_bot() -> String {
    if no_color() {
        String::new()
    } else {
        dim("╰─────╯")
    }
}

const DETAIL_INDENT_NO_COLOR: &str = "     ";

fn styled_line(msg: &str, color_code: &str) -> String {
    if no_color() {
        format!("sgf: {msg}")
    } else {
        format!(
            "{}\n{} {}\n{}",
            badge_top(),
            badge_mid(),
            wrap(color_code, msg, false),
            badge_bot()
        )
    }
}

fn styled_line_detail(msg: &str, color_code: &str, detail: &str) -> String {
    if no_color() {
        format!("sgf: {msg}\n{DETAIL_INDENT_NO_COLOR}{detail}")
    } else {
        format!(
            "{}\n{} {}\n{} {}",
            badge_top(),
            badge_mid(),
            wrap(color_code, msg, false),
            badge_bot(),
            dim(detail)
        )
    }
}

pub fn action(msg: &str) -> String {
    styled_line(msg, "1;37")
}

pub fn action_detail(msg: &str, detail: &str) -> String {
    styled_line_detail(msg, "1;37", detail)
}

pub fn success(msg: &str) -> String {
    styled_line(msg, "1;32")
}

pub fn success_detail(msg: &str, detail: &str) -> String {
    styled_line_detail(msg, "1;32", detail)
}

pub fn warning(msg: &str) -> String {
    styled_line(msg, "1;33")
}

pub fn warning_detail(msg: &str, detail: &str) -> String {
    styled_line_detail(msg, "1;33", detail)
}

pub fn error(msg: &str) -> String {
    styled_line(msg, "1;31")
}

pub fn error_detail(msg: &str, detail: &str) -> String {
    styled_line_detail(msg, "1;31", detail)
}

pub fn print_action(msg: &str) {
    print_box(msg, "1;37");
}

pub fn print_action_detail(msg: &str, detail: &str) {
    print_box_detail(msg, "1;37", detail);
}

pub fn print_success(msg: &str) {
    print_box(msg, "1;32");
}

pub fn print_success_detail(msg: &str, detail: &str) {
    print_box_detail(msg, "1;32", detail);
}

pub fn print_warning(msg: &str) {
    print_box(msg, "1;33");
}

pub fn print_warning_detail(msg: &str, detail: &str) {
    print_box_detail(msg, "1;33", detail);
}

pub fn print_error(msg: &str) {
    print_box(msg, "1;31");
}

pub fn print_error_detail(msg: &str, detail: &str) {
    print_box_detail(msg, "1;31", detail);
}

fn print_box(msg: &str, color_code: &str) {
    if no_color() {
        eprintln!("sgf: {msg}");
    } else {
        eprintln!("{}", badge_top());
        eprintln!("{} {}", badge_mid(), wrap(color_code, msg, false));
        eprintln!("{}", badge_bot());
    }
}

fn print_box_detail(msg: &str, color_code: &str, detail: &str) {
    if no_color() {
        eprintln!("sgf: {msg}");
        eprintln!("{DETAIL_INDENT_NO_COLOR}{detail}");
    } else {
        eprintln!("{}", badge_top());
        eprintln!("{} {}", badge_mid(), wrap(color_code, msg, false));
        eprintln!("{} {}", badge_bot(), dim(detail));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_applies_ansi_code() {
        assert_eq!(wrap("1", "hello", false), "\x1b[1mhello\x1b[0m");
    }

    #[test]
    fn wrap_disabled_returns_plain() {
        assert_eq!(wrap("1", "hello", true), "hello");
    }

    #[test]
    fn wrap_empty_string() {
        assert_eq!(wrap("1", "", false), "\x1b[1m\x1b[0m");
        assert_eq!(wrap("1", "", true), "");
    }

    #[test]
    fn wrap_all_color_codes() {
        assert_eq!(wrap("2", "x", false), "\x1b[2mx\x1b[0m");
        assert_eq!(wrap("31", "x", false), "\x1b[31mx\x1b[0m");
        assert_eq!(wrap("32", "x", false), "\x1b[32mx\x1b[0m");
        assert_eq!(wrap("33", "x", false), "\x1b[33mx\x1b[0m");
        assert_eq!(wrap("37", "x", false), "\x1b[37mx\x1b[0m");
    }

    #[test]
    fn wrap_compound_codes() {
        assert_eq!(wrap("1;7", "sgf", false), "\x1b[1;7msgf\x1b[0m");
        assert_eq!(wrap("1;31", "err", false), "\x1b[1;31merr\x1b[0m");
        assert_eq!(wrap("1;32", "ok", false), "\x1b[1;32mok\x1b[0m");
        assert_eq!(wrap("1;33", "warn", false), "\x1b[1;33mwarn\x1b[0m");
        assert_eq!(wrap("1;37", "act", false), "\x1b[1;37mact\x1b[0m");
    }

    #[test]
    fn wrap_multiline() {
        assert_eq!(wrap("1", "a\nb", false), "\x1b[1ma\nb\x1b[0m");
    }

    #[test]
    fn wrap_utf8() {
        assert_eq!(wrap("31", "héllo", false), "\x1b[31mhéllo\x1b[0m");
    }

    #[test]
    fn badge_top_colored() {
        let out = fmt_badge_top(false);
        assert_eq!(out, "\x1b[2m╭─────╮\x1b[0m");
    }

    #[test]
    fn badge_top_no_color() {
        let out = fmt_badge_top(true);
        assert_eq!(out, "");
    }

    #[test]
    fn badge_mid_colored() {
        let out = fmt_badge_mid(false);
        assert!(out.contains("\x1b[1;7m sgf \x1b[0m"));
        assert!(out.contains("\x1b[2m│\x1b[0m"));
    }

    #[test]
    fn badge_mid_no_color() {
        let out = fmt_badge_mid(true);
        assert_eq!(out, "sgf:");
    }

    #[test]
    fn badge_bot_colored() {
        let out = fmt_badge_bot(false);
        assert_eq!(out, "\x1b[2m╰─────╯\x1b[0m");
    }

    #[test]
    fn badge_bot_no_color() {
        let out = fmt_badge_bot(true);
        assert_eq!(out, "");
    }

    #[test]
    fn action_colored() {
        let out = format_action("launching", false);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], fmt_badge_top(false));
        assert!(lines[1].contains("\x1b[1;7m sgf \x1b[0m"));
        assert!(lines[1].contains("\x1b[1;37mlaunching\x1b[0m"));
        assert_eq!(lines[2], fmt_badge_bot(false));
    }

    #[test]
    fn action_no_color() {
        let out = format_action("launching", true);
        assert_eq!(out, "sgf: launching");
    }

    #[test]
    fn success_colored() {
        let out = format_success("done", false);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[1].contains("\x1b[1;32mdone\x1b[0m"));
    }

    #[test]
    fn success_no_color() {
        let out = format_success("done", true);
        assert_eq!(out, "sgf: done");
    }

    #[test]
    fn warning_colored() {
        let out = format_warning("skipped", false);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[1].contains("\x1b[1;33mskipped\x1b[0m"));
    }

    #[test]
    fn warning_no_color() {
        let out = format_warning("skipped", true);
        assert_eq!(out, "sgf: skipped");
    }

    #[test]
    fn error_colored() {
        let out = format_error("failed", false);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[1].contains("\x1b[1;31mfailed\x1b[0m"));
    }

    #[test]
    fn error_no_color() {
        let out = format_error("failed", true);
        assert_eq!(out, "sgf: failed");
    }

    #[test]
    fn action_detail_colored() {
        let out = format_action_detail("launching", "stage: auth", false);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], fmt_badge_top(false));
        assert!(lines[1].contains("\x1b[1;37mlaunching\x1b[0m"));
        assert!(lines[2].contains("╰─────╯"));
        assert!(lines[2].contains("\x1b[2mstage: auth\x1b[0m"));
    }

    #[test]
    fn action_detail_no_color() {
        let out = format_action_detail("launching", "stage: auth", true);
        assert_eq!(out, "sgf: launching\n     stage: auth");
    }

    #[test]
    fn success_detail_colored() {
        let out = format_success_detail("done", "elapsed: 3s", false);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], fmt_badge_top(false));
        assert!(lines[1].contains("\x1b[1;32mdone\x1b[0m"));
        assert!(lines[2].contains("╰─────╯"));
        assert!(lines[2].contains("\x1b[2melapsed: 3s\x1b[0m"));
    }

    #[test]
    fn success_detail_no_color() {
        let out = format_success_detail("done", "elapsed: 3s", true);
        assert_eq!(out, "sgf: done\n     elapsed: 3s");
    }

    #[test]
    fn warning_detail_colored() {
        let out = format_warning_detail("skipped", "reason: timeout", false);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[1].contains("\x1b[1;33mskipped\x1b[0m"));
        assert!(lines[2].contains("\x1b[2mreason: timeout\x1b[0m"));
    }

    #[test]
    fn warning_detail_no_color() {
        let out = format_warning_detail("skipped", "reason: timeout", true);
        assert_eq!(out, "sgf: skipped\n     reason: timeout");
    }

    #[test]
    fn error_detail_colored() {
        let out = format_error_detail("failed", "exit code: 1", false);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[1].contains("\x1b[1;31mfailed\x1b[0m"));
        assert!(lines[2].contains("\x1b[2mexit code: 1\x1b[0m"));
    }

    #[test]
    fn error_detail_no_color() {
        let out = format_error_detail("failed", "exit code: 1", true);
        assert_eq!(out, "sgf: failed\n     exit code: 1");
    }

    #[test]
    fn strip_ansi_removes_codes() {
        assert_eq!(strip_ansi("\x1b[1mhello\x1b[0m"), "hello");
        assert_eq!(strip_ansi("\x1b[31mred\x1b[0m"), "red");
        assert_eq!(strip_ansi("\x1b[2mdim\x1b[0m text"), "dim text");
    }

    #[test]
    fn strip_ansi_no_codes() {
        assert_eq!(strip_ansi("plain text"), "plain text");
        assert_eq!(strip_ansi(""), "");
    }

    #[test]
    fn strip_ansi_nested_styles() {
        let styled = format!(
            "{} {}",
            wrap("1;7", " sgf ", false),
            wrap("1;32", "done", false),
        );
        assert_eq!(strip_ansi(&styled), " sgf  done");
    }

    #[test]
    fn strip_ansi_preserves_utf8() {
        assert_eq!(strip_ansi("\x1b[1mhéllo\x1b[0m wörld"), "héllo wörld");
    }

    #[test]
    fn strip_ansi_compound_codes() {
        assert_eq!(strip_ansi("\x1b[1;7m sgf \x1b[0m"), " sgf ");
        assert_eq!(strip_ansi("\x1b[1;31merror\x1b[0m"), "error");
    }

    #[test]
    fn action_mid_line_strips_clean() {
        let out = format_action("test msg", false);
        let mid = out.lines().nth(1).unwrap();
        let stripped = strip_ansi(mid);
        assert_eq!(stripped, "│ sgf │ test msg");
    }

    #[test]
    fn detail_indent_aligns_with_message_text() {
        let out = format_action_detail("hello", "info", false);
        let lines: Vec<&str> = out.lines().collect();
        let mid_stripped = strip_ansi(lines[1]);
        let bot_stripped = strip_ansi(lines[2]);
        let msg_start = mid_stripped.chars().position(|c| c == 'h').unwrap();
        let det_start = bot_stripped.chars().position(|c| c == 'i').unwrap();
        assert_eq!(msg_start, det_start);
    }

    // Test helpers that bypass the global OnceLock for deterministic testing
    fn fmt_badge_top(disabled: bool) -> String {
        if disabled {
            String::new()
        } else {
            wrap("2", "╭─────╮", false)
        }
    }

    fn fmt_badge_mid(disabled: bool) -> String {
        if disabled {
            "sgf:".to_string()
        } else {
            format!(
                "{}\x1b[1;7m sgf \x1b[0m{}",
                wrap("2", "│", false),
                wrap("2", "│", false)
            )
        }
    }

    fn fmt_badge_bot(disabled: bool) -> String {
        if disabled {
            String::new()
        } else {
            wrap("2", "╰─────╯", false)
        }
    }

    fn format_styled(msg: &str, color_code: &str, disabled: bool) -> String {
        if disabled {
            format!("sgf: {msg}")
        } else {
            format!(
                "{}\n{} {}\n{}",
                fmt_badge_top(false),
                fmt_badge_mid(false),
                wrap(color_code, msg, false),
                fmt_badge_bot(false)
            )
        }
    }

    fn format_styled_detail(msg: &str, color_code: &str, detail: &str, disabled: bool) -> String {
        if disabled {
            format!("sgf: {msg}\n     {detail}")
        } else {
            format!(
                "{}\n{} {}\n{} {}",
                fmt_badge_top(false),
                fmt_badge_mid(false),
                wrap(color_code, msg, false),
                fmt_badge_bot(false),
                wrap("2", detail, false)
            )
        }
    }

    fn format_action(msg: &str, disabled: bool) -> String {
        format_styled(msg, "1;37", disabled)
    }

    fn format_success(msg: &str, disabled: bool) -> String {
        format_styled(msg, "1;32", disabled)
    }

    fn format_warning(msg: &str, disabled: bool) -> String {
        format_styled(msg, "1;33", disabled)
    }

    fn format_error(msg: &str, disabled: bool) -> String {
        format_styled(msg, "1;31", disabled)
    }

    fn format_action_detail(msg: &str, detail: &str, disabled: bool) -> String {
        format_styled_detail(msg, "1;37", detail, disabled)
    }

    fn format_success_detail(msg: &str, detail: &str, disabled: bool) -> String {
        format_styled_detail(msg, "1;32", detail, disabled)
    }

    fn format_warning_detail(msg: &str, detail: &str, disabled: bool) -> String {
        format_styled_detail(msg, "1;33", detail, disabled)
    }

    fn format_error_detail(msg: &str, detail: &str, disabled: bool) -> String {
        format_styled_detail(msg, "1;31", detail, disabled)
    }
}
