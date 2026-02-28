use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub struct ParseEnumError(pub String);

impl fmt::Display for ParseEnumError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid enum value: {}", self.0)
    }
}

impl std::error::Error for ParseEnumError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueType {
    Bug,
    Task,
    Test,
    Chore,
}

impl IssueType {
    pub fn as_str(&self) -> &'static str {
        match self {
            IssueType::Bug => "bug",
            IssueType::Task => "task",
            IssueType::Test => "test",
            IssueType::Chore => "chore",
        }
    }
}

impl FromStr for IssueType {
    type Err = ParseEnumError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "bug" => Ok(IssueType::Bug),
            "task" => Ok(IssueType::Task),
            "test" => Ok(IssueType::Test),
            "chore" => Ok(IssueType::Chore),
            _ => Err(ParseEnumError(s.to_string())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Open,
    InProgress,
    Closed,
}

impl Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Status::Open => "open",
            Status::InProgress => "in_progress",
            Status::Closed => "closed",
        }
    }
}

impl FromStr for Status {
    type Err = ParseEnumError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "open" => Ok(Status::Open),
            "in_progress" => Ok(Status::InProgress),
            "closed" => Ok(Status::Closed),
            _ => Err(ParseEnumError(s.to_string())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    P0,
    P1,
    P2,
    P3,
}

impl Priority {
    pub fn as_str(&self) -> &'static str {
        match self {
            Priority::P0 => "p0",
            Priority::P1 => "p1",
            Priority::P2 => "p2",
            Priority::P3 => "p3",
        }
    }
}

impl FromStr for Priority {
    type Err = ParseEnumError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "p0" => Ok(Priority::P0),
            "p1" => Ok(Priority::P1),
            "p2" => Ok(Priority::P2),
            "p3" => Ok(Priority::P3),
            _ => Err(ParseEnumError(s.to_string())),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub issue_type: IssueType,
    pub status: Status,
    pub priority: Priority,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fixes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    pub id: String,
    pub issue_id: String,
    pub actor: String,
    pub text: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueDetail {
    #[serde(flatten)]
    pub issue: Issue,
    pub deps: Vec<Issue>,
    pub comments: Vec<Comment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: i64,
    pub issue_id: String,
    pub event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dep {
    pub issue_id: String,
    pub depends_on_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepTreeNode {
    pub id: String,
    pub title: String,
    pub status: Status,
    pub priority: Priority,
    pub issue_type: IssueType,
    pub depth: i32,
}

#[derive(Debug, Clone)]
pub struct CreateIssueParams {
    pub title: String,
    pub issue_type: IssueType,
    pub priority: Priority,
    pub description: Option<String>,
    pub spec: Option<String>,
    pub fixes: Option<String>,
    pub assignee: Option<String>,
    pub deps: Vec<String>,
    pub actor: String,
}

#[derive(Debug, Clone, Default)]
pub struct UpdateFields {
    pub title: Option<String>,
    pub description: Option<String>,
    pub priority: Option<Priority>,
    pub status: Option<Status>,
    pub assignee: Option<String>,
    pub spec: Option<String>,
    pub fixes: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ListFilters {
    pub status: Option<Status>,
    pub priority: Option<Priority>,
    pub assignee: Option<String>,
    pub issue_type: Option<IssueType>,
    pub spec: Option<String>,
    pub sort: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CountResult {
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupedCountResult {
    pub total: i64,
    pub groups: Vec<CountGroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CountGroup {
    pub key: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusEntry {
    pub issue_type: IssueType,
    pub open: i64,
    pub in_progress: i64,
    pub closed: i64,
}
