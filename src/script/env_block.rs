// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Wiltxxx

use std::path::Path;

use crate::cli::{Cli, PathLayout};
use crate::context::TermuxEnv;
use crate::env;

use super::quote::shell_quote;

/// Append `extra` (colon-separated) to `base`, optionally deduplicating
/// while preserving first occurrence. Empty segments are dropped.
fn append_unique(base: &str, extra: &str, dedupe: bool) -> String {
    if extra.is_empty() {
        return base.to_string();
    }
    let combined = format!("{base}:{extra}");
    if !dedupe {
        return combined;
    }
    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<&str> = Vec::new();
    for p in combined.split(':') {
        if !p.is_empty() && seen.insert(p) {
            out.push(p);
        }
    }
    out.join(":")
}

/// The three variables that the shell script actually exports:
/// `(PATH, LD_LIBRARY_PATH, LD_PRELOAD)`.  `-P` / `-L` /
/// `--export-paths` / `--export-ld-lib-paths` are applied on top of the
/// `PathLayout` base.
pub fn compute_env(
    layout: PathLayout,
    prefix: &str,
    sdk: u32,
    cli: &Cli,
) -> (String, String, String) {
    let tbin = format!("{prefix}/bin");
    let tapp = format!("{prefix}/bin/applets");
    let tlib = format!("{prefix}/lib");
    let tpre = format!("{prefix}/lib/libtermux-exec.so");
    let android = env::android_paths(sdk);
    let pre_exists = Path::new(&tpre).is_file();

    // Base PATH and whether termux libs/preload are in play
    let (mut path, use_lib, use_pre) = match layout {
        PathLayout::TermuxFull => (format!("{tbin}:{tapp}:{android}"), sdk < 24, pre_exists),
        PathLayout::TermuxMinimal => (tbin.clone(), sdk < 24, pre_exists),
        PathLayout::TermuxBinOnly => (format!("{tbin}:{tapp}"), sdk < 24, pre_exists),
        PathLayout::AndroidFull => (format!("{android}:{tbin}:{tapp}"), sdk < 24, false),
        PathLayout::AndroidSystemOnly => ("/system/bin".into(), false, false),
        PathLayout::AndroidBinOnly => (android.to_string(), false, false),
    };

    // -P / --export-paths
    if cli.export_all_path {
        if let Ok(v) = std::env::var("PATH") {
            if !v.is_empty() {
                path = append_unique(&path, &v, true);
            }
        }
    }
    if let Some(extra) = &cli.export_paths {
        path = append_unique(&path, extra, true);
    }
    // Normalise away empty segments left by user input.
    path = path
        .split(':')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(":");

    // LD_LIBRARY_PATH
    let mut ld_lib = if use_lib { tlib } else { String::new() };
    if cli.export_all_ld_lib {
        if let Ok(v) = std::env::var("LD_LIBRARY_PATH") {
            if !v.is_empty() {
                ld_lib = append_unique(&ld_lib, &v, true);
            }
        }
    }
    if let Some(extra) = &cli.export_ld_lib_paths {
        ld_lib = append_unique(&ld_lib, extra, true);
    }

    let pre = if use_pre { tpre } else { String::new() };
    (path, ld_lib, pre)
}

/// `(SUDO__TERMUX_PRIORITY_PATH, SUDO__ANDROID_PRIORITY_PATH,
///   SUDO__TERMUX_PRIORITY_LD_LIBRARY_PATH, SUDO__ANDROID_PRIORITY_LD_LIBRARY_PATH)`
///
/// These back the `tpath()` / `apath()` helper functions that the
/// interactive shell can use to switch priority mid-session.  They are
/// **independent** of `-t/-a` (which only sets the initial PATH): they
/// reflect the `-T/-TT/-A/-AA` "secondary paths" semantics, matching
/// what sudo calls `SET_SECONDARY_PATHS`.
pub fn compute_env_paths(
    layout: PathLayout,
    prefix: &str,
    sdk: u32,
    cli: &Cli,
) -> (String, String, String, String) {
    let tbin = format!("{prefix}/bin");
    let tapp = format!("{prefix}/bin/applets");
    let tlib = format!("{prefix}/lib");
    let android = env::android_paths(sdk);

    // Which of the two sides gets the "full" listing depends on the
    // layout family: full layouts keep secondary paths, minimal layouts
    // (-T / -A) collapse to a single directory.
    let secondary = matches!(
        layout,
        PathLayout::TermuxFull
            | PathLayout::AndroidFull
            | PathLayout::TermuxBinOnly
            | PathLayout::AndroidBinOnly
    );

    let (mut tpath, mut apath) = if secondary {
        (
            format!("{tbin}:{tapp}:{android}"),
            format!("{android}:{tbin}:{tapp}"),
        )
    } else {
        (tbin.clone(), "/system/bin".to_string())
    };

    // User extras are appended to *both*, matching sudo's
    // `SUDO_ADDITIONAL_PATHS_TO_EXPORT` behaviour.
    if let Some(extra) = &cli.export_paths {
        tpath = append_unique(&tpath, extra, true);
        apath = append_unique(&apath, extra, true);
    }

    let (tld, ald) = if sdk < 24 {
        // tpath always carries the termux lib; apath only when the
        // layout kept secondary paths (mirrors sudo).
        (tlib.clone(), if secondary { tlib } else { String::new() })
    } else {
        (String::new(), String::new())
    };

    (tpath, apath, tld, ald)
}

/// Emit the `export ...` / `unset ...` lines that set up the root
/// environment.  Deliberately **does not** include hold/sleep/title –
/// those are per-invocation hooks appended by the individual script
/// builders (see `script::epilogue`).
pub fn build_env_block(layout: PathLayout, env: &TermuxEnv, cli: &Cli) -> String {
    let (path, lib, pre) = compute_env(layout, &env.prefix, env.sdk, cli);
    let prefix = &env.prefix;

    let mut s = String::new();
    s.push_str(&format!("export PATH={}\n", shell_quote(&path)));
    if lib.is_empty() {
        s.push_str("unset LD_LIBRARY_PATH\n");
    } else {
        s.push_str(&format!("export LD_LIBRARY_PATH={}\n", shell_quote(&lib)));
    }
    if pre.is_empty() {
        s.push_str("unset LD_PRELOAD\n");
    } else {
        s.push_str(&format!("export LD_PRELOAD={}\n", shell_quote(&pre)));
    }

    s.push_str(&format!("export TERMUX__PREFIX={}\n", shell_quote(prefix)));
    s.push_str(&format!(
        "export TERMUX__HOME={}\n",
        shell_quote(&env.termux_home)
    ));
    s.push_str(&format!(
        "export TERMUX__ROOTFS_DIR={}\n",
        shell_quote(&env.rootfs)
    ));
    s.push_str(&format!("export PREFIX={}\n", shell_quote(prefix)));
    s.push_str(&format!("export TMPDIR={}/tmp\n", prefix));

    s.push_str(&format!("__rsudo_home={}\n", shell_quote(&env.root_home)));
    s.push_str(
        r#"if [ ! -d "$__rsudo_home" ]; then
  mkdir -p "$__rsudo_home" || { echo "rsudo: cannot create root home '$__rsudo_home'" >&2; exit 1; }
fi
if [ "$__rsudo_home" != "$TERMUX__HOME" ]; then
  chown root:root "$__rsudo_home" 2>/dev/null || true
  chmod 700 "$__rsudo_home" 2>/dev/null || true
  case "$__rsudo_home" in
    /sdcard/*|/storage/*|/mnt/sdcard/*)
      echo "rsudo: warning: root home '$__rsudo_home' is on external storage" >&2 ;;
  esac
fi
export HOME="$__rsudo_home"
unset __rsudo_home
"#,
    );

    s.push_str("export TERM=\"${TERM:-xterm-256color}\"\n");
    s.push_str("export LANG=\"${LANG:-en_US.UTF-8}\"\n");
    s
}
