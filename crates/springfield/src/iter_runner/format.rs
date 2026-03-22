use serde::Deserialize;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum StreamEvent {
    Assistant {
        message: AssistantMessage,
    },
    Result {
        result: String,
        #[serde(default)]
        #[allow(dead_code)]
        session_id: Option<String>,
        #[serde(default)]
        usage: Option<Usage>,
    },
    User {
        message: UserMessage,
    },
    System {},
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
struct AssistantMessage {
    content: Vec<ContentBlock>,
}

#[derive(Deserialize)]
struct UserMessage {
    content: Vec<UserContentBlock>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        name: String,
        input: serde_json::Value,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum UserContentBlock {
    ToolResult {
        #[serde(default)]
        content: Option<serde_json::Value>,
        #[serde(default)]
        is_error: Option<bool>,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
struct Usage {
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
}

#[derive(Debug, PartialEq)]
pub enum FormattedOutput {
    Text(String),
    ToolCalls(Vec<FormattedToolCall>),
    ToolResults(Vec<FormattedToolResult>),
    Usage {
        input_tokens: u64,
        output_tokens: u64,
    },
    Result(String),
    Skip,
}

#[derive(Debug, PartialEq)]
pub struct FormattedToolCall {
    pub name: String,
    pub detail: String,
}

#[derive(Debug, PartialEq)]
pub struct FormattedToolResult {
    pub lines: Vec<String>,
    pub is_error: bool,
    pub truncated_count: usize,
}

const MAX_TOOL_RESULT_LINES: usize = 15;

pub fn format_line(line: &str) -> FormattedOutput {
    if !line.starts_with('{') {
        return FormattedOutput::Skip;
    }

    let event: StreamEvent = match serde_json::from_str(line) {
        Ok(e) => e,
        Err(e) => {
            tracing::debug!(error = %e, "skipping malformed JSON line");
            return FormattedOutput::Skip;
        }
    };

    match event {
        StreamEvent::Assistant { message } => {
            let mut texts = Vec::new();
            let mut tool_calls = Vec::new();

            for block in message.content {
                match block {
                    ContentBlock::Text { text } => texts.push(text),
                    ContentBlock::ToolUse { name, input } => {
                        let detail = format_tool_detail(&name, &input);
                        tool_calls.push(FormattedToolCall { name, detail });
                    }
                    ContentBlock::Unknown => {}
                }
            }

            if !tool_calls.is_empty() {
                FormattedOutput::ToolCalls(tool_calls)
            } else if !texts.is_empty() {
                FormattedOutput::Text(texts.join("\n"))
            } else {
                FormattedOutput::Skip
            }
        }
        StreamEvent::Result {
            result: _,
            usage:
                Some(Usage {
                    input_tokens: Some(input),
                    output_tokens: Some(output),
                }),
            ..
        } => FormattedOutput::Usage {
            input_tokens: input,
            output_tokens: output,
        },
        StreamEvent::Result { result, .. } => FormattedOutput::Result(result),
        StreamEvent::User { message } => {
            let results: Vec<FormattedToolResult> = message
                .content
                .into_iter()
                .filter_map(|block| match block {
                    UserContentBlock::ToolResult { content, is_error } => {
                        let text = extract_tool_result_text(&content);
                        let is_error = is_error.unwrap_or(false);
                        let all_lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
                        let total = all_lines.len();
                        let truncated_count = total.saturating_sub(MAX_TOOL_RESULT_LINES);
                        let lines: Vec<String> =
                            all_lines.into_iter().take(MAX_TOOL_RESULT_LINES).collect();
                        Some(FormattedToolResult {
                            lines,
                            is_error,
                            truncated_count,
                        })
                    }
                    UserContentBlock::Unknown => None,
                })
                .collect();

            if results.is_empty() {
                FormattedOutput::Skip
            } else {
                FormattedOutput::ToolResults(results)
            }
        }
        StreamEvent::System {} => {
            tracing::debug!("skipping system event");
            FormattedOutput::Skip
        }
        StreamEvent::Unknown => {
            tracing::debug!("skipping unknown event type");
            FormattedOutput::Skip
        }
    }
}

fn extract_tool_result_text(content: &Option<serde_json::Value>) -> String {
    match content {
        None => String::new(),
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|item| {
                if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                    item.get("text")
                        .and_then(|t| t.as_str())
                        .map(|s| s.to_string())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Some(other) => other.to_string(),
    }
}

fn format_tool_detail(name: &str, input: &serde_json::Value) -> String {
    match name {
        "Read" => {
            let file_path = input["file_path"].as_str().unwrap_or("?");
            let offset = input.get("offset").and_then(|v| v.as_u64());
            let limit = input.get("limit").and_then(|v| v.as_u64());
            match (offset, limit) {
                (Some(o), Some(l)) => format!("{file_path} {o}:{l}"),
                (Some(o), None) => format!("{file_path} {o}"),
                (None, Some(l)) => format!("{file_path} :{l}"),
                (None, None) => file_path.to_string(),
            }
        }
        "Edit" | "Write" => {
            let file_path = input["file_path"].as_str().unwrap_or("?");
            file_path.to_string()
        }
        "Bash" => {
            let command = input["command"].as_str().unwrap_or("?");
            truncate(command, 100)
        }
        "Glob" => {
            let pattern = input["pattern"].as_str().unwrap_or("?");
            pattern.to_string()
        }
        "Grep" => {
            let pattern = input["pattern"].as_str().unwrap_or("?");
            pattern.to_string()
        }
        "TodoWrite" => {
            let count = input
                .get("todos")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            format!("{count} items")
        }
        _ => fallback_detail(input),
    }
}

fn fallback_detail(input: &serde_json::Value) -> String {
    if let Some(obj) = input.as_object() {
        for value in obj.values() {
            if let Some(s) = value.as_str() {
                return truncate(s, 80);
            }
        }
    }
    String::new()
}

pub fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        let char_count = s.chars().count();
        if char_count <= max {
            return s.to_string();
        }
    }

    let mut end = 0;
    for (i, (byte_idx, _)) in s.char_indices().enumerate() {
        if i >= max {
            break;
        }
        end = byte_idx;
    }
    let slice_end = s[end..]
        .chars()
        .next()
        .map(|c| end + c.len_utf8())
        .unwrap_or(end);
    format!("{}...", &s[..slice_end])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tc(name: &str, detail: &str) -> FormattedToolCall {
        FormattedToolCall {
            name: name.into(),
            detail: detail.into(),
        }
    }

    #[test]
    fn text_block_passthrough() {
        let line =
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Hello world"}]}}"#;
        assert_eq!(
            format_line(line),
            FormattedOutput::Text("Hello world".into())
        );
    }

    #[test]
    fn read_tool_basic() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"file_path":"/foo/bar.rs"}}]}}"#;
        assert_eq!(
            format_line(line),
            FormattedOutput::ToolCalls(vec![tc("Read", "/foo/bar.rs")])
        );
    }

    #[test]
    fn read_tool_with_offset_and_limit() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"file_path":"/foo/bar.rs","offset":430,"limit":80}}]}}"#;
        assert_eq!(
            format_line(line),
            FormattedOutput::ToolCalls(vec![tc("Read", "/foo/bar.rs 430:80")])
        );
    }

    #[test]
    fn edit_tool_shows_only_path() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Edit","input":{"file_path":"/foo/bar.rs","old_string":"fn old()","new_string":"fn new()","replace_all":false}}]}}"#;
        assert_eq!(
            format_line(line),
            FormattedOutput::ToolCalls(vec![tc("Edit", "/foo/bar.rs")])
        );
    }

    #[test]
    fn write_tool_shows_only_path() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Write","input":{"file_path":"/foo/new.rs","content":"fn main() {}"}}]}}"#;
        assert_eq!(
            format_line(line),
            FormattedOutput::ToolCalls(vec![tc("Write", "/foo/new.rs")])
        );
    }

    #[test]
    fn bash_tool_shows_command() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash","input":{"command":"git status"}}]}}"#;
        assert_eq!(
            format_line(line),
            FormattedOutput::ToolCalls(vec![tc("Bash", "git status")])
        );
    }

    #[test]
    fn bash_tool_truncates_long_command() {
        let long_cmd = "a".repeat(150);
        let line = format!(
            r#"{{"type":"assistant","message":{{"content":[{{"type":"tool_use","name":"Bash","input":{{"command":"{long_cmd}"}}}}]}}}}"#
        );
        let output = format_line(&line);
        match output {
            FormattedOutput::ToolCalls(calls) => {
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].name, "Bash");
                assert!(calls[0].detail.ends_with("..."));
            }
            other => panic!("expected ToolCalls, got {:?}", other),
        }
    }

    #[test]
    fn glob_tool_shows_pattern() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Glob","input":{"pattern":"specs/**/*.md"}}]}}"#;
        assert_eq!(
            format_line(line),
            FormattedOutput::ToolCalls(vec![tc("Glob", "specs/**/*.md")])
        );
    }

    #[test]
    fn grep_tool_shows_pattern() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Grep","input":{"pattern":"GgufModelBuilder"}}]}}"#;
        assert_eq!(
            format_line(line),
            FormattedOutput::ToolCalls(vec![tc("Grep", "GgufModelBuilder")])
        );
    }

    #[test]
    fn todowrite_shows_item_count() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"TodoWrite","input":{"todos":[{"content":"a","status":"pending"},{"content":"b","status":"pending"},{"content":"c","status":"pending"}]}}]}}"#;
        assert_eq!(
            format_line(line),
            FormattedOutput::ToolCalls(vec![tc("TodoWrite", "3 items")])
        );
    }

    #[test]
    fn unknown_tool_fallback() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"WebSearch","input":{"query":"rust serde"}}]}}"#;
        assert_eq!(
            format_line(line),
            FormattedOutput::ToolCalls(vec![tc("WebSearch", "rust serde")])
        );
    }

    #[test]
    fn result_returns_text() {
        let line = r#"{"type":"result","result":"Done. Updated the file."}"#;
        assert_eq!(
            format_line(line),
            FormattedOutput::Result("Done. Updated the file.".into())
        );
    }

    #[test]
    fn result_with_usage() {
        let line = r#"{"type":"result","result":"Done.","session_id":"sess-123","usage":{"input_tokens":12450,"output_tokens":1230}}"#;
        assert_eq!(
            format_line(line),
            FormattedOutput::Usage {
                input_tokens: 12450,
                output_tokens: 1230,
            }
        );
    }

    #[test]
    fn result_with_partial_usage_returns_result() {
        let line = r#"{"type":"result","result":"Done.","usage":{"input_tokens":100}}"#;
        assert_eq!(format_line(line), FormattedOutput::Result("Done.".into()));
    }

    #[test]
    fn user_event_with_tool_result() {
        let line = r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"line1\nline2\nline3","is_error":false}]}}"#;
        match format_line(line) {
            FormattedOutput::ToolResults(results) => {
                assert_eq!(results.len(), 1);
                assert!(!results[0].is_error);
                assert_eq!(results[0].lines, vec!["line1", "line2", "line3"]);
                assert_eq!(results[0].truncated_count, 0);
            }
            other => panic!("expected ToolResults, got {:?}", other),
        }
    }

    #[test]
    fn user_event_with_error_tool_result() {
        let line = r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"Error: file not found","is_error":true}]}}"#;
        match format_line(line) {
            FormattedOutput::ToolResults(results) => {
                assert_eq!(results.len(), 1);
                assert!(results[0].is_error);
                assert_eq!(results[0].lines, vec!["Error: file not found"]);
            }
            other => panic!("expected ToolResults, got {:?}", other),
        }
    }

    #[test]
    fn user_event_tool_result_truncation() {
        let long_content = (1..=20)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let line = format!(
            r#"{{"type":"user","message":{{"content":[{{"type":"tool_result","content":{}}}]}}}}"#,
            serde_json::to_string(&long_content).unwrap()
        );
        match format_line(&line) {
            FormattedOutput::ToolResults(results) => {
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].lines.len(), MAX_TOOL_RESULT_LINES);
                assert_eq!(results[0].truncated_count, 5);
                assert_eq!(results[0].lines[0], "line 1");
                assert_eq!(results[0].lines[14], "line 15");
            }
            other => panic!("expected ToolResults, got {:?}", other),
        }
    }

    #[test]
    fn user_event_with_array_content() {
        let line = r#"{"type":"user","message":{"content":[{"type":"tool_result","content":[{"type":"text","text":"hello from array"}]}]}}"#;
        match format_line(line) {
            FormattedOutput::ToolResults(results) => {
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].lines, vec!["hello from array"]);
            }
            other => panic!("expected ToolResults, got {:?}", other),
        }
    }

    #[test]
    fn system_event_returns_skip() {
        let line = r#"{"type":"system"}"#;
        assert_eq!(format_line(line), FormattedOutput::Skip);
    }

    #[test]
    fn non_json_line_returns_skip() {
        assert_eq!(format_line("some random text"), FormattedOutput::Skip);
    }

    #[test]
    fn unknown_event_type_returns_skip() {
        let line = r#"{"type":"something_new","data":"whatever"}"#;
        assert_eq!(format_line(line), FormattedOutput::Skip);
    }

    #[test]
    fn malformed_json_returns_skip() {
        let line = r#"{"type":"assistant","broken"#;
        assert_eq!(format_line(line), FormattedOutput::Skip);
    }

    #[test]
    fn multiple_tool_calls() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"file_path":"/a.rs"}},{"type":"tool_use","name":"Read","input":{"file_path":"/b.rs"}}]}}"#;
        assert_eq!(
            format_line(line),
            FormattedOutput::ToolCalls(vec![tc("Read", "/a.rs"), tc("Read", "/b.rs"),])
        );
    }

    #[test]
    fn truncate_respects_utf8() {
        let s = "é".repeat(50);
        let truncated = truncate(&s, 10);
        assert!(truncated.ends_with("..."));
        let without_dots = &truncated[..truncated.len() - 3];
        assert_eq!(without_dots.chars().count(), 10);
    }

    #[test]
    fn truncate_short_string_unchanged() {
        assert_eq!(truncate("hello", 100), "hello");
    }

    #[test]
    fn empty_content_returns_skip() {
        let line = r#"{"type":"assistant","message":{"content":[]}}"#;
        assert_eq!(format_line(line), FormattedOutput::Skip);
    }
}
