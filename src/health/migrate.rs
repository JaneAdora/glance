//! One-time, non-destructive import of today's water glasses and peon-ping reps
//! into the health store. Guarded by a marker so it runs once. Imports DATA only;
//! goals come from config::load_or_seed and are never overridden here.
use crate::health::store;
use std::path::PathBuf;

fn marker_path() -> PathBuf {
    dirs::data_local_dir()
        .map(|d| d.join("glance").join(".health-migrated"))
        .unwrap_or_else(|| PathBuf::from("/tmp/.glance-health-migrated"))
}

pub fn run_once() {
    let marker = marker_path();
    if marker.exists() {
        return;
    }
    import_today_water();
    import_today_peon_reps();
    if let Some(p) = marker.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let _ = std::fs::write(&marker, "1");
}

fn import_today_water() {
    let Some(path) = dirs::data_local_dir().map(|d| d.join("glance").join("water.json")) else {
        return;
    };
    let Ok(s) = std::fs::read_to_string(&path) else {
        return;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) else {
        return;
    };
    let date = v.get("date").and_then(|d| d.as_str()).unwrap_or("");
    let glasses = v.get("glasses").and_then(|g| g.as_u64()).unwrap_or(0);
    let today = store::today_iso();
    if date == today && glasses > 0 {
        store::log_event("water", glasses as f64, &today);
    }
}

fn import_today_peon_reps() {
    let Some(home) = dirs::home_dir() else {
        return;
    };
    let path = home.join(".claude/hooks/peon-ping/.state.json");
    let Ok(s) = std::fs::read_to_string(&path) else {
        return;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) else {
        return;
    };
    let Some(trainer) = v.get("trainer") else {
        return;
    };
    let date = trainer.get("date").and_then(|d| d.as_str()).unwrap_or("");
    let today = store::today_iso();
    if date != today {
        return;
    }
    if let Some(reps) = trainer.get("reps").and_then(|r| r.as_object()) {
        for (name, val) in reps {
            if let Some(n) = val.as_f64() {
                if n > 0.0 {
                    store::log_event(name, n, &today);
                }
            }
        }
    }
}
