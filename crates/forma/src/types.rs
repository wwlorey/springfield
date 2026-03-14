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
pub enum Status {
    Draft,
    Stable,
    Proven,
}

impl Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Status::Draft => "draft",
            Status::Stable => "stable",
            Status::Proven => "proven",
        }
    }
}

impl FromStr for Status {
    type Err = ParseEnumError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "draft" => Ok(Status::Draft),
            "stable" => Ok(Status::Stable),
            "proven" => Ok(Status::Proven),
            _ => Err(ParseEnumError(s.to_string())),
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionKind {
    Required,
    Custom,
}

impl SectionKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SectionKind::Required => "required",
            SectionKind::Custom => "custom",
        }
    }
}

impl FromStr for SectionKind {
    type Err = ParseEnumError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "required" => Ok(SectionKind::Required),
            "custom" => Ok(SectionKind::Custom),
            _ => Err(ParseEnumError(s.to_string())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequiredSection {
    Overview,
    Architecture,
    Dependencies,
    ErrorHandling,
    Testing,
}

impl RequiredSection {
    pub const ALL: [RequiredSection; 5] = [
        RequiredSection::Overview,
        RequiredSection::Architecture,
        RequiredSection::Dependencies,
        RequiredSection::ErrorHandling,
        RequiredSection::Testing,
    ];

    pub fn position(self) -> i64 {
        match self {
            RequiredSection::Overview => 0,
            RequiredSection::Architecture => 1,
            RequiredSection::Dependencies => 2,
            RequiredSection::ErrorHandling => 3,
            RequiredSection::Testing => 4,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            RequiredSection::Overview => "Overview",
            RequiredSection::Architecture => "Architecture",
            RequiredSection::Dependencies => "Dependencies",
            RequiredSection::ErrorHandling => "Error Handling",
            RequiredSection::Testing => "Testing",
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            RequiredSection::Overview => "overview",
            RequiredSection::Architecture => "architecture",
            RequiredSection::Dependencies => "dependencies",
            RequiredSection::ErrorHandling => "error-handling",
            RequiredSection::Testing => "testing",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Spec {
    pub stem: String,
    pub crate_path: String,
    pub purpose: String,
    pub status: Status,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecDetail {
    #[serde(flatten)]
    pub spec: Spec,
    pub sections: Vec<Section>,
    pub refs: Vec<Spec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Section {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec_stem: Option<String>,
    pub name: String,
    pub slug: String,
    pub kind: SectionKind,
    pub body: String,
    pub position: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ref {
    pub from_stem: String,
    pub to_stem: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: i64,
    pub spec_stem: String,
    pub event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug)]
pub enum FormaError {
    NotFound(String),
    AlreadyExists(String),
    CycleDetected,
    RequiredSection(String),
    ValidationFailed(String),
    Internal(String),
}

impl FormaError {
    pub fn code(&self) -> &'static str {
        match self {
            FormaError::NotFound(_) => "not_found",
            FormaError::AlreadyExists(_) => "already_exists",
            FormaError::CycleDetected => "cycle_detected",
            FormaError::RequiredSection(_) => "required_section",
            FormaError::ValidationFailed(_) => "validation_failed",
            FormaError::Internal(_) => "internal",
        }
    }
}

impl fmt::Display for FormaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FormaError::NotFound(msg) => write!(f, "{msg}"),
            FormaError::AlreadyExists(msg) => write!(f, "{msg}"),
            FormaError::CycleDetected => write!(f, "adding this reference would create a cycle"),
            FormaError::RequiredSection(slug) => {
                write!(f, "cannot remove required section: {slug}")
            }
            FormaError::ValidationFailed(msg) => write!(f, "{msg}"),
            FormaError::Internal(msg) => write!(f, "internal error: {msg}"),
        }
    }
}

impl std::error::Error for FormaError {}

pub fn slugify(name: &str) -> String {
    name.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("Overview"), "overview");
        assert_eq!(slugify("Error Handling"), "error-handling");
        assert_eq!(
            slugify("NDJSON Stream Formatting"),
            "ndjson-stream-formatting"
        );
    }

    #[test]
    fn slugify_strips_non_alphanumeric() {
        assert_eq!(slugify("Hello, World!"), "hello-world");
        assert_eq!(slugify("foo@bar#baz"), "foobarbaz");
    }

    #[test]
    fn slugify_multiple_spaces() {
        assert_eq!(slugify("  lots   of   spaces  "), "lots-of-spaces");
    }

    #[test]
    fn slugify_empty() {
        assert_eq!(slugify(""), "");
    }

    #[test]
    fn status_roundtrip() {
        for status in [Status::Draft, Status::Stable, Status::Proven] {
            let s = status.as_str();
            let parsed: Status = s.parse().unwrap();
            assert_eq!(parsed, status);
        }
    }

    #[test]
    fn status_serde_roundtrip() {
        for status in [Status::Draft, Status::Stable, Status::Proven] {
            let json = serde_json::to_string(&status).unwrap();
            let parsed: Status = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, status);
        }
    }

    #[test]
    fn status_invalid() {
        assert!("invalid".parse::<Status>().is_err());
    }

    #[test]
    fn section_kind_roundtrip() {
        for kind in [SectionKind::Required, SectionKind::Custom] {
            let s = kind.as_str();
            let parsed: SectionKind = s.parse().unwrap();
            assert_eq!(parsed, kind);
        }
    }

    #[test]
    fn required_section_positions() {
        assert_eq!(RequiredSection::Overview.position(), 0);
        assert_eq!(RequiredSection::Architecture.position(), 1);
        assert_eq!(RequiredSection::Dependencies.position(), 2);
        assert_eq!(RequiredSection::ErrorHandling.position(), 3);
        assert_eq!(RequiredSection::Testing.position(), 4);
    }

    #[test]
    fn required_section_slugs_match_slugify() {
        for rs in RequiredSection::ALL {
            assert_eq!(slugify(rs.name()), rs.slug());
        }
    }

    #[test]
    fn required_section_all_has_five() {
        assert_eq!(RequiredSection::ALL.len(), 5);
    }

    #[test]
    fn forma_error_codes() {
        assert_eq!(FormaError::NotFound("x".into()).code(), "not_found");
        assert_eq!(
            FormaError::AlreadyExists("x".into()).code(),
            "already_exists"
        );
        assert_eq!(FormaError::CycleDetected.code(), "cycle_detected");
        assert_eq!(
            FormaError::RequiredSection("x".into()).code(),
            "required_section"
        );
        assert_eq!(
            FormaError::ValidationFailed("x".into()).code(),
            "validation_failed"
        );
        assert_eq!(FormaError::Internal("x".into()).code(), "internal");
    }

    #[test]
    fn spec_serde_roundtrip() {
        let spec = Spec {
            stem: "auth".to_string(),
            crate_path: "crates/auth/".to_string(),
            purpose: "Authentication".to_string(),
            status: Status::Draft,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let json = serde_json::to_string(&spec).unwrap();
        let parsed: Spec = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.stem, spec.stem);
        assert_eq!(parsed.status, spec.status);
    }

    #[test]
    fn ref_serde_roundtrip() {
        let r = Ref {
            from_stem: "auth".to_string(),
            to_stem: "ralph".to_string(),
        };
        let json = serde_json::to_string(&r).unwrap();
        let parsed: Ref = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.from_stem, "auth");
        assert_eq!(parsed.to_stem, "ralph");
    }
}
