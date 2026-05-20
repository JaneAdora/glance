//! Optional panel-order configuration: ~/.config/glance/panels.toml
//!
//! ```toml
//! panels = ["clock", "weather", "cpu", "mem", ...]
//! ```
//!
//! The order determines slot assignment (1-9, then 0, then n/p). Only listed
//! panels appear. Absent file → full default registry in default order.
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Default)]
struct PanelsConfig {
    panels: Option<Vec<String>>,
}

pub fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("glance").join("panels.toml"))
}

/// Returns the configured panel order, or None if no (valid) config exists.
pub fn load_order() -> Option<Vec<String>> {
    let path = config_path()?;
    let raw = std::fs::read_to_string(&path).ok()?;
    let cfg: PanelsConfig = toml::from_str(&raw).ok()?;
    cfg.panels.filter(|v| !v.is_empty())
}

/// Write a template panels.toml listing every available panel in default order.
/// Returns the path written.
pub fn write_template(all_panels: &[&str], default_order: &[&str]) -> std::io::Result<PathBuf> {
    let path = config_path()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no config dir"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut body = String::new();
    body.push_str("# glance panel configuration\n");
    body.push_str("# Order sets slot assignment: first 9 -> keys 1-9, 10th -> key 0,\n");
    body.push_str("# the rest are reachable with n / p. Only listed panels appear;\n");
    body.push_str("# comment a line out to hide that panel.\n");
    body.push_str("#\n");
    body.push_str("# Available panels:\n");
    body.push_str("#   ");
    body.push_str(&all_panels.join(", "));
    body.push_str("\n\n");
    body.push_str("panels = [\n");
    for name in default_order {
        body.push_str(&format!("  \"{name}\",\n"));
    }
    // List panels not in default order as commented suggestions (e.g. battery).
    for name in all_panels {
        if !default_order.contains(name) {
            body.push_str(&format!("  # \"{name}\",\n"));
        }
    }
    body.push_str("]\n");
    std::fs::write(&path, body)?;
    Ok(path)
}
