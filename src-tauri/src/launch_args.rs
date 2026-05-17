//! Parses launch-time CLI flags injected by the autostart plugin.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LaunchArgs {
    pub from_autostart: bool,
}

pub fn parse_launch_args<I, S>(args: I) -> LaunchArgs
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut from_autostart = false;
    for a in args {
        if a.as_ref() == "--launched-by-autostart" {
            from_autostart = true;
        }
    }
    LaunchArgs { from_autostart }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_flag_means_manual_launch() {
        let args = parse_launch_args(["fastclaude.exe"]);
        assert!(!args.from_autostart);
    }

    #[test]
    fn flag_present_means_autostart_launch() {
        let args = parse_launch_args(["fastclaude.exe", "--launched-by-autostart"]);
        assert!(args.from_autostart);
    }

    #[test]
    fn flag_anywhere_in_args_is_detected() {
        let args = parse_launch_args([
            "fastclaude.exe",
            "--some-other-flag",
            "--launched-by-autostart",
            "extra",
        ]);
        assert!(args.from_autostart);
    }

    #[test]
    fn unrelated_args_are_ignored() {
        let args = parse_launch_args(["fastclaude.exe", "--unrelated", "--launched-by-something-else"]);
        assert!(!args.from_autostart);
    }

    #[test]
    fn empty_args_is_manual_launch() {
        let empty: [&str; 0] = [];
        let args = parse_launch_args(empty);
        assert!(!args.from_autostart);
    }
}
