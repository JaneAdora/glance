//! Health activity configuration: ~/.config/glance/health.toml
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub struct Activity {
    pub name: String,
    pub goal: f64,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub weekly_target: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct HealthConfig {
    #[serde(rename = "activity", default)]
    pub activities: Vec<Activity>,
}

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .map(|d| d.join("glance").join("health.toml"))
        .unwrap_or_else(|| PathBuf::from("/tmp/glance-health.toml"))
}

/// The starter activity set seeded on first run.
pub fn starter() -> HealthConfig {
    let mk = |name: &str, goal: f64, unit: &str| Activity {
        name: name.into(),
        goal,
        unit: unit.into(),
        weekly_target: None,
    };
    HealthConfig {
        activities: vec![
            mk("pushups", 10.0, "reps"),
            mk("squats", 10.0, "reps"),
            mk("bike", 30.0, "minutes"),
            mk("walking", 30.0, "minutes"),
            mk("water", 8.0, "glasses"),
        ],
    }
}

/// Load the config; if absent/empty/invalid, seed the starter set and write it.
pub fn load_or_seed() -> HealthConfig {
    let path = config_path();
    if let Ok(s) = std::fs::read_to_string(&path) {
        if let Ok(cfg) = toml::from_str::<HealthConfig>(&s) {
            if !cfg.activities.is_empty() {
                return cfg;
            }
        }
    }
    let cfg = starter();
    let _ = write_config(&path, &cfg);
    cfg
}

pub fn write_config(path: &Path, cfg: &HealthConfig) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut body = String::from("# glance health goals. Edit freely; one [[activity]] per goal.\n\n");
    for a in &cfg.activities {
        body.push_str("[[activity]]\n");
        body.push_str(&format!("name = \"{}\"\n", a.name));
        body.push_str(&format!("goal = {}\n", fmt_count(a.goal)));
        body.push_str(&format!("unit = \"{}\"\n", a.unit));
        if let Some(w) = a.weekly_target {
            body.push_str(&format!("weekly_target = {}\n", fmt_count(w)));
        }
        body.push('\n');
    }
    std::fs::write(path, body)
}

/// Format an f64 count without a trailing `.0` (10.0 -> "10", 2.5 -> "2.5").
pub fn fmt_count(v: f64) -> String {
    if v.fract().abs() < 1e-9 {
        format!("{}", v as i64)
    } else {
        let s = format!("{:.2}", v);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starter_has_five_activities() {
        let c = starter();
        assert_eq!(c.activities.len(), 5);
        assert_eq!(c.activities[0].name, "pushups");
        assert_eq!(c.activities[0].goal, 10.0);
        assert_eq!(c.activities[2].unit, "minutes");
    }

    #[test]
    fn parses_toml() {
        let s = r#"
            [[activity]]
            name = "pushups"
            goal = 10
            unit = "reps"

            [[activity]]
            name = "bike"
            goal = 30
            unit = "minutes"
            weekly_target = 150
        "#;
        let c: HealthConfig = toml::from_str(s).unwrap();
        assert_eq!(c.activities.len(), 2);
        assert_eq!(c.activities[1].weekly_target, Some(150.0));
    }

    #[test]
    fn fmt_count_drops_trailing_zero() {
        assert_eq!(fmt_count(10.0), "10");
        assert_eq!(fmt_count(2.5), "2.5");
        assert_eq!(fmt_count(0.0), "0");
        assert_eq!(fmt_count(8.0), "8");
    }
}
