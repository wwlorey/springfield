use reqwest::blocking::Client as HttpClient;
use serde_json::Value;

use crate::types::FormaError;

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
        let base_url = Self::resolve_url();
        let http = HttpClient::new();
        Client { http, base_url }
    }

    pub fn with_url(base_url: String) -> Self {
        let http = HttpClient::new();
        Client { http, base_url }
    }

    fn resolve_url() -> String {
        if let Ok(host) = std::env::var("FM_DAEMON_HOST")
            && !host.trim().is_empty()
        {
            let port = Self::discover_port();
            return format!("http://{host}:{port}");
        }
        if let Ok(url) = std::env::var("FM_DAEMON") {
            return url;
        }
        if let Ok(url) = Self::read_daemon_url() {
            return url;
        }
        let port = Self::discover_port();
        format!("http://localhost:{port}")
    }

    fn read_daemon_url() -> Result<String, ()> {
        let dir = std::env::current_dir().map_err(|_| ())?;
        let url_file = dir.join(".forma/daemon.url");
        let contents = std::fs::read_to_string(&url_file).map_err(|_| ())?;
        let trimmed = contents.trim().to_string();
        if trimmed.is_empty() {
            return Err(());
        }
        Ok(trimmed)
    }

    fn discover_port() -> u16 {
        let dir = std::env::current_dir().unwrap();
        let port_file = dir.join(".forma/daemon.port");
        if let Ok(contents) = std::fs::read_to_string(&port_file)
            && let Ok(port) = contents.trim().parse::<u16>()
        {
            return port;
        }
        crate::db::project_port(&dir)
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

    fn parse_error(resp: reqwest::blocking::Response) -> FormaError {
        #[derive(serde::Deserialize)]
        struct ErrorResponse {
            error: String,
            code: String,
        }

        if let Ok(err_resp) = resp.json::<ErrorResponse>() {
            match err_resp.code.as_str() {
                "not_found" => FormaError::NotFound(err_resp.error),
                "already_exists" => FormaError::AlreadyExists(err_resp.error),
                "cycle_detected" => FormaError::CycleDetected,
                "required_section" => FormaError::RequiredSection(err_resp.error),
                "validation_failed" => FormaError::ValidationFailed(err_resp.error),
                _ => FormaError::Internal(err_resp.error),
            }
        } else {
            FormaError::Internal("unknown error from daemon".to_string())
        }
    }

    fn request<F>(&self, make_request: F) -> Result<Value, FormaError>
    where
        F: FnOnce(&HttpClient) -> reqwest::Result<reqwest::blocking::Response>,
    {
        let resp = make_request(&self.http).map_err(|e| FormaError::Internal(e.to_string()))?;
        if resp.status().is_success() {
            resp.json().map_err(|e| FormaError::Internal(e.to_string()))
        } else {
            Err(Self::parse_error(resp))
        }
    }

    pub fn create_spec(
        &self,
        stem: &str,
        src: Option<&str>,
        purpose: &str,
        actor: &str,
    ) -> Result<Value, FormaError> {
        let mut body = serde_json::json!({
            "stem": stem,
            "purpose": purpose,
        });
        if let Some(s) = src {
            body["src"] = Value::String(s.to_string());
        }
        self.request(|http| {
            http.post(format!("{}/specs", self.base_url))
                .header("x-forma-actor", actor)
                .json(&body)
                .send()
        })
    }

    pub fn get_spec(&self, stem: &str) -> Result<Value, FormaError> {
        self.request(|http| http.get(format!("{}/specs/{}", self.base_url, stem)).send())
    }

    pub fn list_specs(&self, status: Option<&str>) -> Result<Value, FormaError> {
        self.request(|http| {
            let mut req = http.get(format!("{}/specs", self.base_url));
            if let Some(s) = status {
                req = req.query(&[("status", s)]);
            }
            req.send()
        })
    }

    pub fn update_spec(
        &self,
        stem: &str,
        status: Option<&str>,
        src: Option<&str>,
        purpose: Option<&str>,
        actor: &str,
    ) -> Result<Value, FormaError> {
        let mut body = serde_json::Map::new();
        if let Some(s) = status {
            body.insert("status".into(), Value::String(s.to_string()));
        }
        if let Some(s) = src {
            body.insert("src".into(), Value::String(s.to_string()));
        }
        if let Some(p) = purpose {
            body.insert("purpose".into(), Value::String(p.to_string()));
        }

        self.request(|http| {
            http.patch(format!("{}/specs/{}", self.base_url, stem))
                .header("x-forma-actor", actor)
                .json(&Value::Object(body.clone()))
                .send()
        })
    }

    pub fn delete_spec(&self, stem: &str, force: bool) -> Result<Value, FormaError> {
        self.request(|http| {
            let mut url = format!("{}/specs/{}", self.base_url, stem);
            if force {
                url.push_str("?force=true");
            }
            http.delete(&url).send()
        })
    }

    pub fn search_specs(&self, query: &str) -> Result<Value, FormaError> {
        self.request(|http| {
            http.get(format!("{}/specs/search", self.base_url))
                .query(&[("q", query)])
                .send()
        })
    }

    pub fn count_specs(&self, by_status: bool) -> Result<Value, FormaError> {
        self.request(|http| {
            let mut req = http.get(format!("{}/specs/count", self.base_url));
            if by_status {
                req = req.query(&[("by_status", "true")]);
            }
            req.send()
        })
    }

    pub fn project_status(&self) -> Result<Value, FormaError> {
        self.request(|http| http.get(format!("{}/status", self.base_url)).send())
    }

    pub fn spec_history(&self, stem: &str) -> Result<Value, FormaError> {
        self.request(|http| {
            http.get(format!("{}/specs/{}/history", self.base_url, stem))
                .send()
        })
    }

    pub fn add_section(
        &self,
        stem: &str,
        name: &str,
        body: &str,
        after: Option<&str>,
        actor: &str,
    ) -> Result<Value, FormaError> {
        let mut json_body = serde_json::json!({
            "name": name,
            "body": body,
        });
        if let Some(a) = after {
            json_body["after"] = Value::String(a.to_string());
        }
        self.request(|http| {
            http.post(format!("{}/specs/{}/sections", self.base_url, stem))
                .header("x-forma-actor", actor)
                .json(&json_body)
                .send()
        })
    }

    pub fn set_section(
        &self,
        stem: &str,
        slug: &str,
        body: &str,
        actor: &str,
    ) -> Result<Value, FormaError> {
        let json_body = serde_json::json!({ "body": body });
        self.request(|http| {
            http.put(format!(
                "{}/specs/{}/sections/{}",
                self.base_url, stem, slug
            ))
            .header("x-forma-actor", actor)
            .json(&json_body)
            .send()
        })
    }

    pub fn get_section(&self, stem: &str, slug: &str) -> Result<Value, FormaError> {
        self.request(|http| {
            http.get(format!(
                "{}/specs/{}/sections/{}",
                self.base_url, stem, slug
            ))
            .send()
        })
    }

    pub fn list_sections(&self, stem: &str) -> Result<Value, FormaError> {
        self.request(|http| {
            http.get(format!("{}/specs/{}/sections", self.base_url, stem))
                .send()
        })
    }

    pub fn remove_section(&self, stem: &str, slug: &str, actor: &str) -> Result<Value, FormaError> {
        self.request(|http| {
            http.delete(format!(
                "{}/specs/{}/sections/{}",
                self.base_url, stem, slug
            ))
            .header("x-forma-actor", actor)
            .send()
        })
    }

    pub fn move_section(
        &self,
        stem: &str,
        slug: &str,
        after: &str,
        actor: &str,
    ) -> Result<Value, FormaError> {
        let json_body = serde_json::json!({ "after": after });
        self.request(|http| {
            http.patch(format!(
                "{}/specs/{}/sections/{}/move",
                self.base_url, stem, slug
            ))
            .header("x-forma-actor", actor)
            .json(&json_body)
            .send()
        })
    }

    pub fn add_ref(&self, stem: &str, target: &str, actor: &str) -> Result<Value, FormaError> {
        let json_body = serde_json::json!({ "target": target });
        self.request(|http| {
            http.post(format!("{}/specs/{}/refs", self.base_url, stem))
                .header("x-forma-actor", actor)
                .json(&json_body)
                .send()
        })
    }

    pub fn remove_ref(&self, stem: &str, target: &str, actor: &str) -> Result<Value, FormaError> {
        self.request(|http| {
            http.delete(format!("{}/specs/{}/refs/{}", self.base_url, stem, target))
                .header("x-forma-actor", actor)
                .send()
        })
    }

    pub fn list_refs(&self, stem: &str) -> Result<Value, FormaError> {
        self.request(|http| {
            http.get(format!("{}/specs/{}/refs", self.base_url, stem))
                .send()
        })
    }

    pub fn ref_tree(&self, stem: &str, direction: &str) -> Result<Value, FormaError> {
        self.request(|http| {
            http.get(format!("{}/specs/{}/refs/tree", self.base_url, stem))
                .query(&[("direction", direction)])
                .send()
        })
    }

    pub fn ref_cycles(&self) -> Result<Value, FormaError> {
        self.request(|http| http.get(format!("{}/refs/cycles", self.base_url)).send())
    }

    pub fn export(&self) -> Result<Value, FormaError> {
        self.request(|http| http.post(format!("{}/export", self.base_url)).send())
    }

    pub fn import(&self) -> Result<Value, FormaError> {
        self.request(|http| http.post(format!("{}/import", self.base_url)).send())
    }

    pub fn check(&self) -> Result<Value, FormaError> {
        self.request(|http| http.get(format!("{}/check", self.base_url)).send())
    }

    pub fn doctor(&self, fix: bool) -> Result<Value, FormaError> {
        self.request(|http| {
            let mut url = format!("{}/doctor", self.base_url);
            if fix {
                url.push_str("?fix=true");
            }
            http.post(&url).send()
        })
    }
}
