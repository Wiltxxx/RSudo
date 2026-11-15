// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Wiltxxx

use crate::cli::{Cli, Kind, PathLayout, Priority, SearchMode};

pub fn decide_kind(cli: &Cli) -> (Kind, &[String]) {
    if cli.script_mode {
        return (Kind::Script, &cli.command[..]);
    }
    if cli.force_path {
        return (Kind::Exec, &cli.command[..]);
    }
    match cli.command.first().map(|s| s.as_str()) {
        Some("su") => (Kind::Su, &cli.command[1..]),
        Some("asu") => (Kind::Asu, &cli.command[1..]),
        _ => (Kind::Exec, &cli.command[..]),
    }
}

pub fn decide_interactive(cli: &Cli, kind: Kind, args: &[String]) -> bool {
    if cli.script_mode {
        return false;
    }
    let has_interactive_hint = cli.interactive || cli.shell.is_some() || cli.shell_home.is_some();
    !cli.force_path
        && (matches!(kind, Kind::Su | Kind::Asu) || (has_interactive_hint && args.is_empty()))
}

pub fn decide_interactive_priority(cli: &Cli, kind: Kind) -> Priority {
    if cli.termux_only > 0 || cli.termux_priority || matches!(kind, Kind::Su) {
        Priority::Termux
    } else {
        Priority::Android
    }
}

pub fn decide_search_mode(cli: &Cli) -> (SearchMode, Option<Priority>) {
    if cli.termux_only > 0 || cli.termux_priority {
        (SearchMode::TermuxFirst, Some(Priority::Termux))
    } else if cli.android_only >= 2 {
        (SearchMode::AndroidFirst, Some(Priority::Android))
    } else if cli.android_only == 1 {
        (SearchMode::AndroidOnly, Some(Priority::Android))
    } else if cli.android_priority {
        (SearchMode::AndroidFirst, Some(Priority::Android))
    } else {
        (SearchMode::TermuxFirst, None)
    }
}

/// Map CLI flags + resolved command location into a concrete `PathLayout`.
pub fn decide_layout(cli: &Cli, kind: Kind, resolved_under_rootfs: bool) -> PathLayout {
    // -TT
    if cli.termux_only >= 2 {
        return PathLayout::TermuxBinOnly;
    }
    // -T
    if cli.termux_only == 1 {
        return PathLayout::TermuxMinimal;
    }
    // -AA
    if cli.android_only >= 2 {
        return PathLayout::AndroidBinOnly;
    }
    // -A
    if cli.android_only == 1 {
        return PathLayout::AndroidSystemOnly;
    }

    // default: priority decides order, full set in both cases
    let termux_first = match kind {
        Kind::Su => true,
        Kind::Asu => false,
        Kind::Exec | Kind::Script => {
            if cli.termux_priority {
                true
            } else if cli.android_priority {
                false
            } else {
                resolved_under_rootfs
            }
        }
    };
    if termux_first {
        PathLayout::TermuxFull
    } else {
        PathLayout::AndroidFull
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn parse(argv: &[&str]) -> Cli {
        let mut v = vec!["rsudo"];
        v.extend_from_slice(argv);
        Cli::parse_from(v)
    }

    #[test]
    fn kind_script_via_s() {
        let cli = parse(&["-s", "echo hi"]);
        assert_eq!(decide_kind(&cli).0, Kind::Script);
    }

    #[test]
    fn kind_su_still_works() {
        let cli = parse(&["su"]);
        assert_eq!(decide_kind(&cli).0, Kind::Su);
    }

    #[test]
    fn capital_t_sets_minimal() {
        let cli = parse(&["-T", "ls"]);
        assert_eq!(
            decide_layout(&cli, Kind::Exec, true),
            PathLayout::TermuxMinimal
        );
    }

    #[test]
    fn double_capital_t_sets_bin_only() {
        let cli = parse(&["-TT", "ls"]);
        assert_eq!(
            decide_layout(&cli, Kind::Exec, true),
            PathLayout::TermuxBinOnly
        );
    }

    #[test]
    fn capital_a_sets_system_only() {
        let cli = parse(&["-A", "ls"]);
        assert_eq!(
            decide_layout(&cli, Kind::Exec, false),
            PathLayout::AndroidSystemOnly
        );
    }

    #[test]
    fn double_capital_a_sets_android_bin_only() {
        let cli = parse(&["-AA", "ls"]);
        assert_eq!(
            decide_layout(&cli, Kind::Exec, false),
            PathLayout::AndroidBinOnly
        );
    }

    #[test]
    fn default_exec_under_rootfs_is_termux_full() {
        let cli = parse(&["ls"]);
        assert_eq!(
            decide_layout(&cli, Kind::Exec, true),
            PathLayout::TermuxFull
        );
    }

    #[test]
    fn default_exec_outside_rootfs_is_android_full() {
        let cli = parse(&["ls"]);
        assert_eq!(
            decide_layout(&cli, Kind::Exec, false),
            PathLayout::AndroidFull
        );
    }

    #[test]
    fn script_interactive_is_false() {
        let cli = parse(&["-s", "x"]);
        assert!(!decide_interactive(&cli, Kind::Script, &cli.command));
    }
}
