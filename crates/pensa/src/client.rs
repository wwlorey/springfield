use reqwest::blocking::Client as HttpClient;
use serde_json::Value;

use crate::error::{ErrorResponse, PensaError};
use crate::types::{CreateIssueParams, ListFilters};

pub struct Client {
    http: HttpClient,
    base_url: String,
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

impl Client {
    pub fn new() -> Self {
        let base_url =
            std::env::var("PN_DAEMON").unwrap_or_else(|_| "http://localhost:7533".to_string());
        let http = HttpClient::new();
        Client { http, base_url }
    }

    pub fn check_reachable(&self) -> Result<(), String> {
        match self.http.get(format!("{}/status", self.base_url)).send() {
            Ok(resp) if resp.status().is_success() => Ok(()),
            Ok(resp) => Err(format!("daemon returned status {}", resp.status())),
            Err(e) => Err(format!("cannot reach daemon at {}: {}", self.base_url, e)),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn parse_error(resp: reqwest::blocking::Response) -> PensaError {
        if let Ok(err_resp) = resp.json::<ErrorResponse>() {
            match err_resp.code.as_deref() {
                Some("not_found") => PensaError::NotFound(err_resp.error),
                Some("already_claimed") => PensaError::AlreadyClaimed {
                    id: String::new(),
                    holder: err_resp.error,
                },
                Some("cycle_detected") => PensaError::CycleDetected,
                Some("invalid_status_transition") => PensaError::InvalidStatusTransition {
                    from: String::new(),
                    to: err_resp.error,
                },
                _ => PensaError::Internal(err_resp.error),
            }
        } else {
            PensaError::Internal("unknown error from daemon".to_string())
        }
    }

    pub fn create_issue(&self, params: &CreateIssueParams) -> Result<Value, PensaError> {
        let mut body = serde_json::json!({
            "title": params.title,
            "issue_type": params.issue_type,
            "priority": params.priority,
            "actor": params.actor,
            "deps": params.deps,
        });
        if let Some(ref d) = params.description {
            body["description"] = Value::String(d.clone());
        }
        if let Some(ref s) = params.spec {
            body["spec"] = Value::String(s.clone());
        }
        if let Some(ref f) = params.fixes {
            body["fixes"] = Value::String(f.clone());
        }
        if let Some(ref a) = params.assignee {
            body["assignee"] = Value::String(a.clone());
        }

        let resp = self
            .http
            .post(format!("{}/issues", self.base_url))
            .json(&body)
            .send()
            .map_err(|e| PensaError::Internal(e.to_string()))?;

        if resp.status().is_success() {
            resp.json().map_err(|e| PensaError::Internal(e.to_string()))
        } else {
            Err(Self::parse_error(resp))
        }
    }

    pub fn get_issue(&self, id: &str) -> Result<Value, PensaError> {
        let resp = self
            .http
            .get(format!("{}/issues/{}", self.base_url, id))
            .send()
            .map_err(|e| PensaError::Internal(e.to_string()))?;

        if resp.status().is_success() {
            resp.json().map_err(|e| PensaError::Internal(e.to_string()))
        } else {
            Err(Self::parse_error(resp))
        }
    }

    pub fn update_issue(&self, id: &str, fields: &Value, actor: &str) -> Result<Value, PensaError> {
        let mut body = fields.clone();
        body["actor"] = Value::String(actor.to_string());

        let resp = self
            .http
            .patch(format!("{}/issues/{}", self.base_url, id))
            .json(&body)
            .send()
            .map_err(|e| PensaError::Internal(e.to_string()))?;

        if resp.status().is_success() {
            resp.json().map_err(|e| PensaError::Internal(e.to_string()))
        } else {
            Err(Self::parse_error(resp))
        }
    }

    pub fn delete_issue(&self, id: &str, force: bool) -> Result<(), PensaError> {
        let mut url = format!("{}/issues/{}", self.base_url, id);
        if force {
            url.push_str("?force=true");
        }

        let resp = self
            .http
            .delete(&url)
            .send()
            .map_err(|e| PensaError::Internal(e.to_string()))?;

        if resp.status().is_success() {
            Ok(())
        } else {
            Err(Self::parse_error(resp))
        }
    }

    pub fn close_issue(
        &self,
        id: &str,
        reason: Option<&str>,
        force: bool,
        actor: &str,
    ) -> Result<Value, PensaError> {
        let body = serde_json::json!({
            "reason": reason,
            "force": force,
            "actor": actor,
        });

        let resp = self
            .http
            .post(format!("{}/issues/{}/close", self.base_url, id))
            .json(&body)
            .send()
            .map_err(|e| PensaError::Internal(e.to_string()))?;

        if resp.status().is_success() {
            resp.json().map_err(|e| PensaError::Internal(e.to_string()))
        } else {
            Err(Self::parse_error(resp))
        }
    }

    pub fn reopen_issue(
        &self,
        id: &str,
        reason: Option<&str>,
        actor: &str,
    ) -> Result<Value, PensaError> {
        let body = serde_json::json!({
            "reason": reason,
            "actor": actor,
        });

        let resp = self
            .http
            .post(format!("{}/issues/{}/reopen", self.base_url, id))
            .json(&body)
            .send()
            .map_err(|e| PensaError::Internal(e.to_string()))?;

        if resp.status().is_success() {
            resp.json().map_err(|e| PensaError::Internal(e.to_string()))
        } else {
            Err(Self::parse_error(resp))
        }
    }

    pub fn release_issue(&self, id: &str, actor: &str) -> Result<Value, PensaError> {
        let resp = self
            .http
            .post(format!("{}/issues/{}/release", self.base_url, id))
            .header("x-pensa-actor", actor)
            .send()
            .map_err(|e| PensaError::Internal(e.to_string()))?;

        if resp.status().is_success() {
            resp.json().map_err(|e| PensaError::Internal(e.to_string()))
        } else {
            Err(Self::parse_error(resp))
        }
    }

    pub fn list_issues(&self, filters: &ListFilters) -> Result<Value, PensaError> {
        let mut params = Vec::new();
        if let Some(ref s) = filters.status {
            params.push(("status".to_string(), s.as_str().to_string()));
        }
        if let Some(ref p) = filters.priority {
            params.push(("priority".to_string(), p.as_str().to_string()));
        }
        if let Some(ref a) = filters.assignee {
            params.push(("assignee".to_string(), a.clone()));
        }
        if let Some(ref t) = filters.issue_type {
            params.push(("type".to_string(), t.as_str().to_string()));
        }
        if let Some(ref s) = filters.spec {
            params.push(("spec".to_string(), s.clone()));
        }
        if let Some(ref s) = filters.sort {
            params.push(("sort".to_string(), s.clone()));
        }
        if let Some(l) = filters.limit {
            params.push(("limit".to_string(), l.to_string()));
        }

        let resp = self
            .http
            .get(format!("{}/issues", self.base_url))
            .query(&params)
            .send()
            .map_err(|e| PensaError::Internal(e.to_string()))?;

        if resp.status().is_success() {
            resp.json().map_err(|e| PensaError::Internal(e.to_string()))
        } else {
            Err(Self::parse_error(resp))
        }
    }

    pub fn ready_issues(&self, filters: &ListFilters) -> Result<Value, PensaError> {
        let mut params = Vec::new();
        if let Some(ref p) = filters.priority {
            params.push(("priority".to_string(), p.as_str().to_string()));
        }
        if let Some(ref a) = filters.assignee {
            params.push(("assignee".to_string(), a.clone()));
        }
        if let Some(ref t) = filters.issue_type {
            params.push(("type".to_string(), t.as_str().to_string()));
        }
        if let Some(ref s) = filters.spec {
            params.push(("spec".to_string(), s.clone()));
        }
        if let Some(l) = filters.limit {
            params.push(("limit".to_string(), l.to_string()));
        }

        let resp = self
            .http
            .get(format!("{}/issues/ready", self.base_url))
            .query(&params)
            .send()
            .map_err(|e| PensaError::Internal(e.to_string()))?;

        if resp.status().is_success() {
            resp.json().map_err(|e| PensaError::Internal(e.to_string()))
        } else {
            Err(Self::parse_error(resp))
        }
    }

    pub fn blocked_issues(&self) -> Result<Value, PensaError> {
        let resp = self
            .http
            .get(format!("{}/issues/blocked", self.base_url))
            .send()
            .map_err(|e| PensaError::Internal(e.to_string()))?;

        if resp.status().is_success() {
            resp.json().map_err(|e| PensaError::Internal(e.to_string()))
        } else {
            Err(Self::parse_error(resp))
        }
    }

    pub fn search_issues(&self, query: &str) -> Result<Value, PensaError> {
        let resp = self
            .http
            .get(format!("{}/issues/search", self.base_url))
            .query(&[("q", query)])
            .send()
            .map_err(|e| PensaError::Internal(e.to_string()))?;

        if resp.status().is_success() {
            resp.json().map_err(|e| PensaError::Internal(e.to_string()))
        } else {
            Err(Self::parse_error(resp))
        }
    }

    pub fn count_issues(
        &self,
        by_status: bool,
        by_priority: bool,
        by_issue_type: bool,
        by_assignee: bool,
    ) -> Result<Value, PensaError> {
        let mut params = Vec::new();
        if by_status {
            params.push(("by_status", "true"));
        }
        if by_priority {
            params.push(("by_priority", "true"));
        }
        if by_issue_type {
            params.push(("by_issue_type", "true"));
        }
        if by_assignee {
            params.push(("by_assignee", "true"));
        }

        let resp = self
            .http
            .get(format!("{}/issues/count", self.base_url))
            .query(&params)
            .send()
            .map_err(|e| PensaError::Internal(e.to_string()))?;

        if resp.status().is_success() {
            resp.json().map_err(|e| PensaError::Internal(e.to_string()))
        } else {
            Err(Self::parse_error(resp))
        }
    }

    pub fn project_status(&self) -> Result<Value, PensaError> {
        let resp = self
            .http
            .get(format!("{}/status", self.base_url))
            .send()
            .map_err(|e| PensaError::Internal(e.to_string()))?;

        if resp.status().is_success() {
            resp.json().map_err(|e| PensaError::Internal(e.to_string()))
        } else {
            Err(Self::parse_error(resp))
        }
    }

    pub fn issue_history(&self, id: &str) -> Result<Value, PensaError> {
        let resp = self
            .http
            .get(format!("{}/issues/{}/history", self.base_url, id))
            .send()
            .map_err(|e| PensaError::Internal(e.to_string()))?;

        if resp.status().is_success() {
            resp.json().map_err(|e| PensaError::Internal(e.to_string()))
        } else {
            Err(Self::parse_error(resp))
        }
    }

    pub fn add_dep(
        &self,
        issue_id: &str,
        depends_on_id: &str,
        actor: &str,
    ) -> Result<Value, PensaError> {
        let body = serde_json::json!({
            "issue_id": issue_id,
            "depends_on_id": depends_on_id,
            "actor": actor,
        });

        let resp = self
            .http
            .post(format!("{}/deps", self.base_url))
            .json(&body)
            .send()
            .map_err(|e| PensaError::Internal(e.to_string()))?;

        if resp.status().is_success() {
            resp.json().map_err(|e| PensaError::Internal(e.to_string()))
        } else {
            Err(Self::parse_error(resp))
        }
    }

    pub fn remove_dep(&self, issue_id: &str, depends_on_id: &str) -> Result<Value, PensaError> {
        let resp = self
            .http
            .delete(format!("{}/deps", self.base_url))
            .query(&[("issue_id", issue_id), ("depends_on_id", depends_on_id)])
            .send()
            .map_err(|e| PensaError::Internal(e.to_string()))?;

        if resp.status().is_success() {
            resp.json().map_err(|e| PensaError::Internal(e.to_string()))
        } else {
            Err(Self::parse_error(resp))
        }
    }

    pub fn list_deps(&self, id: &str) -> Result<Value, PensaError> {
        let resp = self
            .http
            .get(format!("{}/issues/{}/deps", self.base_url, id))
            .send()
            .map_err(|e| PensaError::Internal(e.to_string()))?;

        if resp.status().is_success() {
            resp.json().map_err(|e| PensaError::Internal(e.to_string()))
        } else {
            Err(Self::parse_error(resp))
        }
    }

    pub fn dep_tree(&self, id: &str, direction: &str) -> Result<Value, PensaError> {
        let resp = self
            .http
            .get(format!("{}/issues/{}/deps/tree", self.base_url, id))
            .query(&[("direction", direction)])
            .send()
            .map_err(|e| PensaError::Internal(e.to_string()))?;

        if resp.status().is_success() {
            resp.json().map_err(|e| PensaError::Internal(e.to_string()))
        } else {
            Err(Self::parse_error(resp))
        }
    }

    pub fn dep_cycles(&self) -> Result<Value, PensaError> {
        let resp = self
            .http
            .get(format!("{}/deps/cycles", self.base_url))
            .send()
            .map_err(|e| PensaError::Internal(e.to_string()))?;

        if resp.status().is_success() {
            resp.json().map_err(|e| PensaError::Internal(e.to_string()))
        } else {
            Err(Self::parse_error(resp))
        }
    }

    pub fn add_comment(&self, id: &str, text: &str, actor: &str) -> Result<Value, PensaError> {
        let body = serde_json::json!({
            "text": text,
            "actor": actor,
        });

        let resp = self
            .http
            .post(format!("{}/issues/{}/comments", self.base_url, id))
            .json(&body)
            .send()
            .map_err(|e| PensaError::Internal(e.to_string()))?;

        if resp.status().is_success() {
            resp.json().map_err(|e| PensaError::Internal(e.to_string()))
        } else {
            Err(Self::parse_error(resp))
        }
    }

    pub fn list_comments(&self, id: &str) -> Result<Value, PensaError> {
        let resp = self
            .http
            .get(format!("{}/issues/{}/comments", self.base_url, id))
            .send()
            .map_err(|e| PensaError::Internal(e.to_string()))?;

        if resp.status().is_success() {
            resp.json().map_err(|e| PensaError::Internal(e.to_string()))
        } else {
            Err(Self::parse_error(resp))
        }
    }

    pub fn export(&self) -> Result<Value, PensaError> {
        let resp = self
            .http
            .post(format!("{}/export", self.base_url))
            .send()
            .map_err(|e| PensaError::Internal(e.to_string()))?;

        if resp.status().is_success() {
            resp.json().map_err(|e| PensaError::Internal(e.to_string()))
        } else {
            Err(Self::parse_error(resp))
        }
    }

    pub fn import(&self) -> Result<Value, PensaError> {
        let resp = self
            .http
            .post(format!("{}/import", self.base_url))
            .send()
            .map_err(|e| PensaError::Internal(e.to_string()))?;

        if resp.status().is_success() {
            resp.json().map_err(|e| PensaError::Internal(e.to_string()))
        } else {
            Err(Self::parse_error(resp))
        }
    }

    pub fn doctor(&self, fix: bool) -> Result<Value, PensaError> {
        let mut params = Vec::new();
        if fix {
            params.push(("fix", "true"));
        }

        let resp = self
            .http
            .post(format!("{}/doctor", self.base_url))
            .query(&params)
            .send()
            .map_err(|e| PensaError::Internal(e.to_string()))?;

        if resp.status().is_success() {
            resp.json().map_err(|e| PensaError::Internal(e.to_string()))
        } else {
            Err(Self::parse_error(resp))
        }
    }
}
