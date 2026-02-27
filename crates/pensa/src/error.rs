use serde::Serialize;
use std::fmt;

#[derive(Debug)]
pub enum PensaError {
    NotFound(String),
    AlreadyClaimed { id: String, holder: String },
    CycleDetected,
    InvalidStatusTransition { from: String, to: String },
    DeleteRequiresForce(String),
    Internal(String),
}

impl fmt::Display for PensaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PensaError::NotFound(id) => write!(f, "issue not found: {id}"),
            PensaError::AlreadyClaimed { id, holder } => {
                write!(f, "issue {id} already claimed by {holder}")
            }
            PensaError::CycleDetected => write!(f, "adding this dependency would create a cycle"),
            PensaError::InvalidStatusTransition { from, to } => {
                write!(f, "invalid status transition from {from} to {to}")
            }
            PensaError::DeleteRequiresForce(reason) => {
                write!(f, "delete requires --force: {reason}")
            }
            PensaError::Internal(msg) => write!(f, "internal error: {msg}"),
        }
    }
}

impl PensaError {
    pub fn code(&self) -> Option<&'static str> {
        match self {
            PensaError::NotFound(_) => Some("not_found"),
            PensaError::AlreadyClaimed { .. } => Some("already_claimed"),
            PensaError::CycleDetected => Some("cycle_detected"),
            PensaError::InvalidStatusTransition { .. } => Some("invalid_status_transition"),
            PensaError::DeleteRequiresForce(_) => None,
            PensaError::Internal(_) => None,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

impl From<&PensaError> for ErrorResponse {
    fn from(err: &PensaError) -> Self {
        ErrorResponse {
            error: err.to_string(),
            code: err.code().map(String::from),
        }
    }
}
