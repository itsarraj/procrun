/// ANSI foreground color codes cycled across processes so each one's
/// output prefix is visually distinct in a terminal. Deliberately
/// skips red (commonly reserved for actual errors elsewhere) and both
/// black/white variants (poor contrast against light/dark terminal
/// backgrounds respectively).
const PALETTE: [&str; 6] = [
    "\x1b[36m", // cyan
    "\x1b[32m", // green
    "\x1b[33m", // yellow
    "\x1b[35m", // magenta
    "\x1b[34m", // blue
    "\x1b[96m", // bright cyan
];
const RESET: &str = "\x1b[0m";

pub fn color_for_index(index: usize) -> &'static str {
    PALETTE[index % PALETTE.len()]
}

pub fn colorize(text: &str, color: &str) -> String {
    format!("{color}{text}{RESET}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycles_through_the_palette_rather_than_panicking_past_its_length() {
        let a = color_for_index(0);
        let wrapped = color_for_index(PALETTE.len());
        assert_eq!(a, wrapped);
    }

    #[test]
    fn colorize_wraps_text_with_the_given_color_and_a_reset() {
        let out = colorize("web", "\x1b[36m");
        assert!(out.starts_with("\x1b[36m"));
        assert!(out.ends_with(RESET));
        assert!(out.contains("web"));
    }
}
