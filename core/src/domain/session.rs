//! Study tracker session types.

use serde::{Deserialize, Serialize};

/// One recorded study session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StudySession {
    /// Database row id (0 before first save; auto-incremented on insert).
    pub id: i64,
    /// Session timestamp, `YYYY-MM-DD HH:MM:SS`.
    pub date: String,
    /// Duration in fractional hours.
    pub hours: f32,
    /// Free-form activity category.
    pub activity_type: String,
    /// Free-form phase/project label.
    pub phase: String,
    /// Optional notes.
    pub notes: Option<String>,
}

/// A key/value pair persisted by the tracker (gates, project status, config).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackerKv {
    /// Unique key.
    pub key: String,
    /// Stored value.
    pub value: String,
}
