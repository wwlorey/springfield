use std::sync::OnceLock;

static NO_COLOR: OnceLock<bool> = OnceLock::new();

fn no_color() -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bold_styled() {
        assert_eq!(wrap("1", "hello", false), "\x1b[1mhello\x1b[0m");
    }

    #[test]
    fn dim_styled() {
        assert_eq!(wrap("2", "hello", false), "\x1b[2mhello\x1b[0m");
    }

    #[test]
    fn green_styled() {
        assert_eq!(wrap("32", "hello", false), "\x1b[32mhello\x1b[0m");
    }

    #[test]
    fn yellow_styled() {
        assert_eq!(wrap("33", "hello", false), "\x1b[33mhello\x1b[0m");
    }

    #[test]
    fn red_styled() {
        assert_eq!(wrap("31", "hello", false), "\x1b[31mhello\x1b[0m");
    }

    #[test]
    fn no_color_returns_plain_text() {
        assert_eq!(wrap("1", "hello", true), "hello");
        assert_eq!(wrap("2", "hello", true), "hello");
        assert_eq!(wrap("32", "hello", true), "hello");
        assert_eq!(wrap("33", "hello", true), "hello");
        assert_eq!(wrap("31", "hello", true), "hello");
    }

    #[test]
    fn empty_string_styled() {
        assert_eq!(wrap("1", "", false), "\x1b[1m\x1b[0m");
    }

    #[test]
    fn empty_string_no_color() {
        assert_eq!(wrap("1", "", true), "");
    }

    #[test]
    fn multiline_content() {
        assert_eq!(
            wrap("1", "line1\nline2", false),
            "\x1b[1mline1\nline2\x1b[0m"
        );
    }

    #[test]
    fn special_characters() {
        assert_eq!(
            wrap("31", "héllo wörld", false),
            "\x1b[31mhéllo wörld\x1b[0m"
        );
    }
}
