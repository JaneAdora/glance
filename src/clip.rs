//! Shared clipboard helper: OSC 52 (reaches the outer terminal over SSH/tmux)
//! plus wl-copy (local Wayland). Best-effort; failures are silent.
use std::io::Write;
use std::process::Command;

/// Put `s` on the clipboard via OSC 52 and wl-copy.
pub fn copy(s: &str) {
    let seq = format!("\x1b]52;c;{}\x07", b64(s.as_bytes()));
    let mut out = std::io::stdout();
    let _ = out.write_all(seq.as_bytes());
    let _ = out.flush();
    // Silence wl-copy's stdout/stderr: over SSH (no WAYLAND_DISPLAY) it prints a
    // multi-line "failed to connect to a Wayland server" error that, if inherited,
    // lands on the alternate screen and corrupts the TUI.
    if let Ok(mut c) = Command::new("wl-copy")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        if let Some(mut si) = c.stdin.take() {
            let _ = si.write_all(s.as_bytes());
        }
        let _ = c.wait();
    }
}

/// Minimal standard base64 (no line breaks) for OSC 52 payloads.
pub fn b64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(T[(n >> 18 & 63) as usize] as char);
        out.push(T[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6 & 63) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[(n & 63) as usize] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::b64;
    #[test]
    fn b64_matches_known_vectors() {
        assert_eq!(b64(b"gst"), "Z3N0");
        assert_eq!(b64(b"clip"), "Y2xpcA==");
        assert_eq!(b64(b""), "");
    }
}
