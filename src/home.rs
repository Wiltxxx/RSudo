// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Wiltxxx

pub fn expand_path(p: &str, prefix: &str, home: &str) -> String {
    if p == "~" || p == "$HOME" || p == "${HOME}" {
        return home.to_string();
    }
    for (pre, val) in [
        ("~/", home),
        ("$HOME/", home),
        ("${HOME}/", home),
        ("$PREFIX/", prefix),
        ("${PREFIX}/", prefix),
    ] {
        if let Some(r) = p.strip_prefix(pre) {
            return format!("{val}/{r}");
        }
    }
    if p == "$PREFIX" || p == "${PREFIX}" {
        return prefix.to_string();
    }
    p.to_string()
}

pub fn get_root_home(
    spec: Option<&str>,
    prefix: &str,
    termux_home: &str,
) -> Result<String, String> {
    let p = match spec {
        Some(s) => expand_path(s, prefix, termux_home),
        None => format!("{termux_home}/.suroot"),
    };
    if !p.starts_with('/') {
        return Err(format!(
            "--shell-home must be an absolute path after expansion (got {p:?})"
        ));
    }
    Ok(p)
}
