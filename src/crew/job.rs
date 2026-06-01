//! A background Claude Code job, parsed from ~/.claude/jobs/<short>/state.json.
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Deserialize)]
struct Raw {
    #[serde(default)]
    state: String,
    #[serde(default)]
    tempo: String,
    #[serde(default)]
    detail: String,
    #[serde(default)]
    cwd: String,
    #[serde(default, rename = "resumeSessionId")]
    resume_session_id: String,
    #[serde(default, rename = "sessionId")]
    session_id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    intent: String,
    #[serde(default, rename = "updatedAt")]
    updated_at: String,
    #[serde(default, rename = "inFlight")]
    in_flight: InFlight,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct InFlight {
    #[serde(default)]
    tasks: u64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Job {
    pub short: String,
    pub name: String,
    pub state: String,
    pub tempo: String,
    pub in_flight: u64,
    pub detail: String,
    pub cwd: String,
    pub resume_session_id: String,
    pub updated_at: String,
    pub intent: String,
}

pub fn jobs_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/"))
        .join(".claude")
        .join("jobs")
}

/// Parse one state.json string into a Job (short is the dir name).
pub fn parse_job(short: &str, json: &str) -> Option<Job> {
    let r: Raw = serde_json::from_str(json).ok()?;
    let resume = if !r.resume_session_id.is_empty() {
        r.resume_session_id
    } else {
        r.session_id
    };
    Some(Job {
        short: short.to_string(),
        name: r.name,
        state: r.state,
        tempo: r.tempo,
        in_flight: r.in_flight.tasks,
        detail: r.detail,
        cwd: r.cwd,
        resume_session_id: resume,
        updated_at: r.updated_at,
        intent: r.intent,
    })
}

/// Load all jobs, newest-first by updated_at (ISO 8601 sorts lexically), tie-break short.
pub fn load_jobs() -> Vec<Job> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(jobs_dir()) else {
        return out;
    };
    for e in entries.flatten() {
        if !e.path().is_dir() {
            continue;
        }
        let short = e.file_name().to_string_lossy().into_owned();
        let sj = e.path().join("state.json");
        if let Ok(s) = std::fs::read_to_string(&sj) {
            if let Some(j) = parse_job(&short, &s) {
                out.push(j);
            }
        }
    }
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(a.short.cmp(&b.short)));
    out
}

impl Job {
    pub fn is_live(&self) -> bool {
        matches!(self.state.as_str(), "working" | "blocked")
            || self.tempo == "active"
            || self.in_flight > 0
    }

    pub fn display_name(&self) -> String {
        if self.name.trim().is_empty() {
            format!("({})", self.short)
        } else {
            self.name.clone()
        }
    }

    fn resume_id(&self) -> &str {
        if !self.resume_session_id.is_empty() {
            &self.resume_session_id
        } else {
            &self.short
        }
    }

    /// (cwd if non-empty, bare claude command with the dangerous flag).
    pub fn resume_parts(&self) -> (Option<String>, String) {
        let cwd = if self.cwd.trim().is_empty() {
            None
        } else {
            Some(self.cwd.clone())
        };
        let claude = format!(
            "claude --resume {} --dangerously-skip-permissions",
            suite_term::quote::shell_quote(self.resume_id())
        );
        (cwd, claude)
    }

    /// Shell command: `cd <cwd> && claude --resume <id> --dangerously-skip-permissions`
    /// (cwd/id shell-quoted only when they contain metacharacters).
    pub fn resume_command(&self) -> String {
        let (cwd, claude) = self.resume_parts();
        match cwd {
            Some(c) => format!("cd {} && {}", suite_term::quote::shell_quote(&c), claude),
            None => claude,
        }
    }

    /// Humanized age from updated_at to `now` (a jiff Timestamp). "?" if unparseable.
    pub fn age(&self, now: jiff::Timestamp) -> String {
        let Ok(ts) = self.updated_at.parse::<jiff::Timestamp>() else {
            return "?".to_string();
        };
        let secs = (now.as_second() - ts.as_second()).max(0);
        let mins = secs / 60;
        let hours = mins / 60;
        let days = hours / 24;
        if days >= 1 {
            format!("{days}d")
        } else if hours >= 1 {
            format!("{hours}h")
        } else if mins >= 1 {
            format!("{mins}m")
        } else {
            format!("{secs}s")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "state": "working", "tempo": "active",
        "inFlight": {"tasks": 2},
        "name": "R-Suite", "detail": "building health",
        "cwd": "/home/jane", "resumeSessionId": "abc-123",
        "intent": "build a thing", "updatedAt": "2026-05-24T03:00:28.258Z"
    }"#;

    #[test]
    fn parses_sample() {
        let j = parse_job("31006806", SAMPLE).unwrap();
        assert_eq!(j.short, "31006806");
        assert_eq!(j.name, "R-Suite");
        assert_eq!(j.state, "working");
        assert_eq!(j.in_flight, 2);
        assert_eq!(j.resume_session_id, "abc-123");
        assert!(j.is_live());
    }

    #[test]
    fn missing_fields_are_tolerant() {
        let j = parse_job("xyz", "{}").unwrap();
        assert_eq!(j.short, "xyz");
        assert_eq!(j.display_name(), "(xyz)");
        assert!(!j.is_live());
    }

    #[test]
    fn falls_back_to_session_id_then_short() {
        let j = parse_job("s1", r#"{"sessionId":"sess-9"}"#).unwrap();
        assert_eq!(j.resume_session_id, "sess-9");
        let j2 = parse_job("s2", "{}").unwrap();
        assert_eq!(j2.resume_command(), "claude --resume s2 --dangerously-skip-permissions");
    }

    #[test]
    fn is_live_truth_table() {
        let mk = |state: &str, tempo: &str, n: u64| Job {
            state: state.into(), tempo: tempo.into(), in_flight: n, ..Default::default()
        };
        assert!(mk("working", "idle", 0).is_live());
        assert!(mk("blocked", "idle", 0).is_live());
        assert!(mk("done", "active", 0).is_live());
        assert!(mk("done", "idle", 1).is_live());
        assert!(!mk("done", "idle", 0).is_live());
        assert!(!mk("stopped", "idle", 0).is_live());
        assert!(!mk("failed", "idle", 0).is_live());
    }

    #[test]
    fn resume_command_cd_and_escape_and_flag() {
        let j = Job { cwd: "/home/jane".into(), resume_session_id: "abc-123".into(), ..Default::default() };
        assert_eq!(
            j.resume_command(),
            "cd /home/jane && claude --resume abc-123 --dangerously-skip-permissions"
        );
        let j2 = Job { cwd: "/home/jane's dir".into(), resume_session_id: "x".into(), ..Default::default() };
        assert_eq!(
            j2.resume_command(),
            "cd '/home/jane'\\''s dir' && claude --resume x --dangerously-skip-permissions"
        );
    }
}
