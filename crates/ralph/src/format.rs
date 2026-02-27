use serde::Deserialize;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum StreamEvent {
    Assistant {
        message: AssistantMessage,
    },
    Result {
        result: String,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
struct AssistantMessage {
    content: Vec<ContentBlock>,
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

pub fn format_line(line: &str) -> Option<String> {
    if !line.starts_with('{') {
        return None;
    }

    let event: StreamEvent = match serde_json::from_str(line) {
        Ok(e) => e,
        Err(_) => return None,
    };

    match event {
        StreamEvent::Assistant { message } => {
            let parts: Vec<String> = message
                .content
                .into_iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text } => Some(text),
                    ContentBlock::ToolUse { name, input } => Some(format_tool_call(&name, &input)),
                    ContentBlock::Unknown => None,
                })
                .collect();

            if parts.is_empty() {
                None
            } else {
                Some(parts.join("\n"))
            }
        }
        StreamEvent::Result { result } => Some(result),
        StreamEvent::Unknown => None,
    }
}

fn format_tool_call(name: &str, input: &serde_json::Value) -> String {
    let detail = match name {
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
    };

    format!("-> {name}({detail})")
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

pub(crate) fn truncate(s: &str, max: usize) -> String {
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
    // end is the byte index of the last character we want to include
    // We need to advance past it to get the slice end
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

    #[test]
    fn text_block_passthrough() {
        let line =
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Hello world"}]}}"#;
        assert_eq!(format_line(line).unwrap(), "Hello world");
    }

    #[test]
    fn read_tool_basic() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"file_path":"/foo/bar.rs"}}]}}"#;
        assert_eq!(format_line(line).unwrap(), "-> Read(/foo/bar.rs)");
    }

    #[test]
    fn read_tool_with_offset_and_limit() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"file_path":"/foo/bar.rs","offset":430,"limit":80}}]}}"#;
        assert_eq!(format_line(line).unwrap(), "-> Read(/foo/bar.rs 430:80)");
    }

    #[test]
    fn edit_tool_shows_only_path() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Edit","input":{"file_path":"/foo/bar.rs","old_string":"fn old()","new_string":"fn new()","replace_all":false}}]}}"#;
        assert_eq!(format_line(line).unwrap(), "-> Edit(/foo/bar.rs)");
    }

    #[test]
    fn write_tool_shows_only_path() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Write","input":{"file_path":"/foo/new.rs","content":"fn main() {}"}}]}}"#;
        assert_eq!(format_line(line).unwrap(), "-> Write(/foo/new.rs)");
    }

    #[test]
    fn bash_tool_shows_command() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash","input":{"command":"git status"}}]}}"#;
        assert_eq!(format_line(line).unwrap(), "-> Bash(git status)");
    }

    #[test]
    fn bash_tool_truncates_long_command() {
        let long_cmd = "a".repeat(150);
        let line = format!(
            r#"{{"type":"assistant","message":{{"content":[{{"type":"tool_use","name":"Bash","input":{{"command":"{long_cmd}"}}}}]}}}}"#
        );
        let output = format_line(&line).unwrap();
        assert!(output.starts_with("-> Bash("));
        assert!(output.ends_with("...)"));
        let detail = &output["-> Bash(".len()..output.len() - 1];
        assert!(detail.len() <= 103 + 3);
    }

    #[test]
    fn glob_tool_shows_pattern() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Glob","input":{"pattern":"specs/**/*.md"}}]}}"#;
        assert_eq!(format_line(line).unwrap(), "-> Glob(specs/**/*.md)");
    }

    #[test]
    fn grep_tool_shows_pattern() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Grep","input":{"pattern":"GgufModelBuilder"}}]}}"#;
        assert_eq!(format_line(line).unwrap(), "-> Grep(GgufModelBuilder)");
    }

    #[test]
    fn todowrite_shows_item_count() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"TodoWrite","input":{"todos":[{"content":"a","status":"pending"},{"content":"b","status":"pending"},{"content":"c","status":"pending"}]}}]}}"#;
        assert_eq!(format_line(line).unwrap(), "-> TodoWrite(3 items)");
    }

    #[test]
    fn unknown_tool_fallback() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"WebSearch","input":{"query":"rust serde"}}]}}"#;
        assert_eq!(format_line(line).unwrap(), "-> WebSearch(rust serde)");
    }

    #[test]
    fn result_returns_text() {
        let line = r#"{"type":"result","result":"Done. Updated the file."}"#;
        assert_eq!(format_line(line).unwrap(), "Done. Updated the file.");
    }

    #[test]
    fn non_json_line_returns_none() {
        assert!(format_line("some random text").is_none());
    }

    #[test]
    fn unknown_event_type_returns_none() {
        let line = r#"{"type":"system","data":"something"}"#;
        assert!(format_line(line).is_none());
    }

    #[test]
    fn malformed_json_returns_none() {
        let line = r#"{"type":"assistant","broken"#;
        assert!(format_line(line).is_none());
    }

    #[test]
    fn multiple_content_blocks_joined() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"file_path":"/a.rs"}},{"type":"tool_use","name":"Read","input":{"file_path":"/b.rs"}}]}}"#;
        assert_eq!(format_line(line).unwrap(), "-> Read(/a.rs)\n-> Read(/b.rs)");
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
    fn empty_content_returns_none() {
        let line = r#"{"type":"assistant","message":{"content":[]}}"#;
        assert!(format_line(line).is_none());
    }
}
