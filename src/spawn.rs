//! Shared spawn helpers. Currently: open a command in a new tmux window.
//! Extracted from the crew panel so launchers can reuse it.
use std::process::Command;

/// True when running inside a tmux session.
pub fn in_tmux() -> bool {
    std::env::var("TMUX").is_ok()
}

/// Build the argv for `tmux new-window [-c <cwd>] <args...>`. Pure; unit-tested.
pub fn tmux_argv(cwd: Option<&str>, args: &[&str]) -> Vec<String> {
    let mut v = vec!["new-window".to_string()];
    if let Some(d) = cwd {
        v.push("-c".to_string());
        v.push(d.to_string());
    }
    v.extend(args.iter().map(|s| s.to_string()));
    v
}

/// Spawn a new tmux window running `args`. Returns true on success.
/// The caller owns the not-in-tmux fallback; this never falls back.
pub fn tmux_new_window(cwd: Option<&str>, args: &[&str]) -> bool {
    Command::new("tmux")
        .args(tmux_argv(cwd, args))
        // Null the child's stdout/stderr: an inherited tmux error (e.g. "no
        // server running") would land on the caller's alt-screen and corrupt it.
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argv_without_cwd() {
        assert_eq!(tmux_argv(None, &["gst"]), vec!["new-window", "gst"]);
    }

    #[test]
    fn argv_with_cwd() {
        assert_eq!(
            tmux_argv(Some("/home/jane"), &["gst"]),
            vec!["new-window", "-c", "/home/jane", "gst"]
        );
    }

    #[test]
    fn argv_preserves_multi_arg_order() {
        assert_eq!(
            tmux_argv(None, &["claude", "--resume", "abc", "--dangerously-skip-permissions"]),
            vec!["new-window", "claude", "--resume", "abc", "--dangerously-skip-permissions"]
        );
    }
}
