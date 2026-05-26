//! ClaudeTask + Status + type aliases.
use serde::{Deserialize, Serialize};

pub type SessionId = String;
pub type TaskId = String;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pending,
    InProgress,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaudeTask {
    pub id: TaskId,
    pub subject: String,
    #[serde(default)]
    pub description: String,
    #[serde(rename = "activeForm", default)]
    pub active_form: String,
    pub status: Status,
    #[serde(default)]
    pub blocks: Vec<TaskId>,
    #[serde(rename = "blockedBy", default)]
    pub blocked_by: Vec<TaskId>,
}

impl ClaudeTask {
    /// Numeric parse for sort + max-id; unparseable ids sink to the bottom.
    pub fn parse_id(&self) -> u64 {
        self.id.parse::<u64>().unwrap_or(u64::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_real_world_json() {
        let raw = r#"{
            "id": "4",
            "subject": "add validation",
            "description": "Validate the form on submit.",
            "activeForm": "Adding validation",
            "status": "in_progress",
            "blocks": ["5"],
            "blockedBy": ["3"]
        }"#;
        let t: ClaudeTask = serde_json::from_str(raw).unwrap();
        assert_eq!(t.id, "4");
        assert_eq!(t.status, Status::InProgress);
        assert_eq!(t.active_form, "Adding validation");
        assert_eq!(t.blocked_by, vec!["3".to_string()]);
        let again = serde_json::to_string(&t).unwrap();
        let t2: ClaudeTask = serde_json::from_str(&again).unwrap();
        assert_eq!(t, t2);
    }

    #[test]
    fn status_snake_case() {
        assert_eq!(serde_json::to_string(&Status::InProgress).unwrap(), "\"in_progress\"");
        assert_eq!(serde_json::to_string(&Status::Pending).unwrap(), "\"pending\"");
        assert_eq!(serde_json::to_string(&Status::Completed).unwrap(), "\"completed\"");
    }

    #[test]
    fn parse_id_unparseable_sinks() {
        let t = ClaudeTask {
            id: "weird".into(),
            subject: "".into(),
            description: "".into(),
            active_form: "".into(),
            status: Status::Pending,
            blocks: vec![],
            blocked_by: vec![],
        };
        assert_eq!(t.parse_id(), u64::MAX);
    }

    #[test]
    fn missing_optional_fields_defaults() {
        let raw = r#"{"id":"1","subject":"s","status":"pending"}"#;
        let t: ClaudeTask = serde_json::from_str(raw).unwrap();
        assert_eq!(t.description, "");
        assert_eq!(t.blocks.len(), 0);
    }
}
