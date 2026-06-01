//! Shared clipboard helper: OSC 52 (reaches the outer terminal over SSH/tmux)
//! plus wl-copy (local Wayland). Best-effort; failures are silent.
//! Backed by the shared `suite-term` crate.

/// Put `s` on the clipboard via OSC 52 and wl-copy.
pub fn copy(s: &str) {
    suite_term::clipboard::copy(s);
}
