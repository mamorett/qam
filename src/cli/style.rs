use std::io::IsTerminal;

#[derive(Debug, Clone, Copy)]
pub struct Styler {
    plain: bool,
}

impl Styler {
    pub fn new() -> Self {
        Self {
            plain: !Self::is_color_enabled(),
        }
    }

    pub fn plain() -> Self {
        Self { plain: true }
    }

    pub fn with_color(enabled: bool) -> Self {
        Self { plain: !enabled }
    }

    pub fn is_color_enabled() -> bool {
        if std::env::var_os("NO_COLOR").is_some() {
            return false;
        }
        if let Ok(term) = std::env::var("TERM")
            && term == "dumb"
        {
            return false;
        }
        if let Ok(force) = std::env::var("CLICOLOR_FORCE")
            && force != "0"
        {
            return true;
        }
        std::io::stdout().is_terminal()
    }

    pub fn ok(&self, s: &str) -> String {
        if self.plain {
            s.to_string()
        } else {
            // #4caf50 = 76, 175, 80
            format!("\x1b[38;2;76;175;80m{s}\x1b[0m")
        }
    }

    pub fn warn(&self, s: &str) -> String {
        if self.plain {
            s.to_string()
        } else {
            // #ff9800 = 255, 152, 0
            format!("\x1b[38;2;255;152;0m{s}\x1b[0m")
        }
    }

    pub fn err(&self, s: &str) -> String {
        if self.plain {
            s.to_string()
        } else {
            // #e53935 = 229, 57, 53
            format!("\x1b[38;2;229;57;53m{s}\x1b[0m")
        }
    }

    pub fn accent(&self, s: &str) -> String {
        if self.plain {
            s.to_string()
        } else {
            // #00acc1 = 0, 172, 193
            format!("\x1b[38;2;0;172;193m{s}\x1b[0m")
        }
    }
}

impl Default for Styler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plain_styler() {
        let s = Styler::plain();
        assert_eq!(s.ok("success"), "success");
        assert_eq!(s.warn("warning"), "warning");
        assert_eq!(s.err("error"), "error");
        assert_eq!(s.accent("accent"), "accent");
    }

    #[test]
    fn test_colored_styler() {
        let s = Styler { plain: false };
        assert!(s.ok("test").contains("\x1b[38;2;76;175;80m"));
        assert!(s.warn("test").contains("\x1b[38;2;255;152;0m"));
        assert!(s.err("test").contains("\x1b[38;2;229;57;53m"));
        assert!(s.accent("test").contains("\x1b[38;2;0;172;193m"));
    }
}
