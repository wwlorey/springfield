use serde_json::Value;

use crate::error::PensaError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Json,
    Human,
}

pub fn print_json(value: &Value) {
    println!("{}", serde_json::to_string_pretty(value).unwrap());
}

pub fn print_error(err: &PensaError, mode: OutputMode) {
    match mode {
        OutputMode::Json => {
            let resp = crate::error::ErrorResponse::from(err);
            eprintln!("{}", serde_json::to_string(&resp).unwrap());
        }
        OutputMode::Human => {
            eprintln!("error: {err}");
        }
    }
}

pub fn print_issue(value: &Value, mode: OutputMode) {
    match mode {
        OutputMode::Json => print_json(value),
        OutputMode::Human => {
            let id = value["id"].as_str().unwrap_or("?");
            let title = value["title"].as_str().unwrap_or("?");
            let status = value["status"].as_str().unwrap_or("?");
            let priority = value["priority"].as_str().unwrap_or("?");
            let itype = value["issue_type"].as_str().unwrap_or("?");
            let assignee = value["assignee"].as_str().unwrap_or("-");
            println!("{id}  {priority} {status:<11} [{itype}] {title}  @{assignee}");
        }
    }
}

pub fn print_issue_detail(value: &Value, mode: OutputMode) {
    match mode {
        OutputMode::Json => print_json(value),
        OutputMode::Human => {
            let id = value["id"].as_str().unwrap_or("?");
            let title = value["title"].as_str().unwrap_or("?");
            let status = value["status"].as_str().unwrap_or("?");
            let priority = value["priority"].as_str().unwrap_or("?");
            let itype = value["issue_type"].as_str().unwrap_or("?");
            let assignee = value["assignee"].as_str().unwrap_or("-");
            let created = value["created_at"].as_str().unwrap_or("?");

            println!("{id}  [{itype}] {title}");
            println!("  status: {status}  priority: {priority}  assignee: {assignee}");
            println!("  created: {created}");

            if let Some(desc) = value["description"].as_str() {
                println!("  description: {desc}");
            }
            if let Some(spec) = value["spec"].as_str() {
                println!("  spec: {spec}");
            }
            if let Some(fixes) = value["fixes"].as_str() {
                println!("  fixes: {fixes}");
            }

            if let Some(deps) = value["deps"].as_array()
                && !deps.is_empty()
            {
                println!("  deps:");
                for dep in deps {
                    let dep_id = dep["id"].as_str().unwrap_or("?");
                    let dep_title = dep["title"].as_str().unwrap_or("?");
                    let dep_status = dep["status"].as_str().unwrap_or("?");
                    println!("    {dep_id} [{dep_status}] {dep_title}");
                }
            }

            if let Some(comments) = value["comments"].as_array()
                && !comments.is_empty()
            {
                println!("  comments:");
                for c in comments {
                    let actor = c["actor"].as_str().unwrap_or("?");
                    let text = c["text"].as_str().unwrap_or("");
                    let at = c["created_at"].as_str().unwrap_or("?");
                    println!("    [{at}] {actor}: {text}");
                }
            }
        }
    }
}

pub fn print_issue_list(value: &Value, mode: OutputMode) {
    match mode {
        OutputMode::Json => print_json(value),
        OutputMode::Human => {
            if let Some(arr) = value.as_array() {
                if arr.is_empty() {
                    println!("(no issues)");
                } else {
                    for item in arr {
                        print_issue(item, OutputMode::Human);
                    }
                }
            }
        }
    }
}

pub fn print_events(value: &Value, mode: OutputMode) {
    match mode {
        OutputMode::Json => print_json(value),
        OutputMode::Human => {
            if let Some(arr) = value.as_array() {
                if arr.is_empty() {
                    println!("(no events)");
                } else {
                    for ev in arr {
                        let etype = ev["event_type"].as_str().unwrap_or("?");
                        let actor = ev["actor"].as_str().unwrap_or("-");
                        let at = ev["created_at"].as_str().unwrap_or("?");
                        let detail = ev["detail"].as_str().unwrap_or("");
                        if detail.is_empty() {
                            println!("  {at}  {etype} by {actor}");
                        } else {
                            println!("  {at}  {etype} by {actor}: {detail}");
                        }
                    }
                }
            }
        }
    }
}

pub fn print_dep_status(value: &Value, mode: OutputMode) {
    match mode {
        OutputMode::Json => print_json(value),
        OutputMode::Human => {
            let status = value["status"].as_str().unwrap_or("?");
            let issue_id = value["issue_id"].as_str().unwrap_or("?");
            let depends_on = value["depends_on_id"].as_str().unwrap_or("?");
            println!("dep {status}: {issue_id} -> {depends_on}");
        }
    }
}

pub fn print_dep_tree(value: &Value, mode: OutputMode) {
    match mode {
        OutputMode::Json => print_json(value),
        OutputMode::Human => {
            if let Some(arr) = value.as_array() {
                if arr.is_empty() {
                    println!("(no dependencies)");
                } else {
                    for node in arr {
                        let depth = node["depth"].as_i64().unwrap_or(0) as usize;
                        let indent = "  ".repeat(depth);
                        let id = node["id"].as_str().unwrap_or("?");
                        let title = node["title"].as_str().unwrap_or("?");
                        let status = node["status"].as_str().unwrap_or("?");
                        println!("{indent}{id} [{status}] {title}");
                    }
                }
            }
        }
    }
}

pub fn print_cycles(value: &Value, mode: OutputMode) {
    match mode {
        OutputMode::Json => print_json(value),
        OutputMode::Human => {
            if let Some(arr) = value.as_array() {
                if arr.is_empty() {
                    println!("no cycles detected");
                } else {
                    for (i, cycle) in arr.iter().enumerate() {
                        if let Some(ids) = cycle.as_array() {
                            let chain: Vec<&str> = ids.iter().filter_map(|v| v.as_str()).collect();
                            println!("cycle {}: {}", i + 1, chain.join(" -> "));
                        }
                    }
                }
            }
        }
    }
}

pub fn print_comment(value: &Value, mode: OutputMode) {
    match mode {
        OutputMode::Json => print_json(value),
        OutputMode::Human => {
            let actor = value["actor"].as_str().unwrap_or("?");
            let text = value["text"].as_str().unwrap_or("");
            let at = value["created_at"].as_str().unwrap_or("?");
            println!("[{at}] {actor}: {text}");
        }
    }
}

pub fn print_comment_list(value: &Value, mode: OutputMode) {
    match mode {
        OutputMode::Json => print_json(value),
        OutputMode::Human => {
            if let Some(arr) = value.as_array() {
                if arr.is_empty() {
                    println!("(no comments)");
                } else {
                    for c in arr {
                        print_comment(c, OutputMode::Human);
                    }
                }
            }
        }
    }
}

pub fn print_count(value: &Value, mode: OutputMode) {
    match mode {
        OutputMode::Json => print_json(value),
        OutputMode::Human => {
            if let Some(count) = value["count"].as_i64() {
                println!("count: {count}");
            } else if let Some(total) = value["total"].as_i64() {
                println!("total: {total}");
                if let Some(groups) = value["groups"].as_array() {
                    for g in groups {
                        let key = g["key"].as_str().unwrap_or("?");
                        let count = g["count"].as_i64().unwrap_or(0);
                        println!("  {key}: {count}");
                    }
                }
            }
        }
    }
}

pub fn print_status(value: &Value, mode: OutputMode) {
    match mode {
        OutputMode::Json => print_json(value),
        OutputMode::Human => {
            if let Some(arr) = value.as_array() {
                println!(
                    "{:<8} {:>5} {:>11} {:>7}",
                    "type", "open", "in_progress", "closed"
                );
                for entry in arr {
                    let itype = entry["issue_type"].as_str().unwrap_or("?");
                    let open = entry["open"].as_i64().unwrap_or(0);
                    let in_prog = entry["in_progress"].as_i64().unwrap_or(0);
                    let closed = entry["closed"].as_i64().unwrap_or(0);
                    println!("{itype:<8} {open:>5} {in_prog:>11} {closed:>7}");
                }
            }
        }
    }
}

pub fn print_doctor(value: &Value, mode: OutputMode) {
    match mode {
        OutputMode::Json => print_json(value),
        OutputMode::Human => {
            if let Some(findings) = value["findings"].as_array() {
                if findings.is_empty() {
                    println!("no issues found");
                } else {
                    for f in findings {
                        let check = f["check"].as_str().unwrap_or("?");
                        let msg = f["message"].as_str().unwrap_or("");
                        println!("  [{check}] {msg}");
                    }
                }
            }
            if let Some(fixes) = value["fixes_applied"].as_array()
                && !fixes.is_empty()
            {
                println!("fixes applied:");
                for fix in fixes {
                    if let Some(s) = fix.as_str() {
                        println!("  {s}");
                    }
                }
            }
        }
    }
}

pub fn print_export_import(value: &Value, mode: OutputMode) {
    match mode {
        OutputMode::Json => print_json(value),
        OutputMode::Human => {
            let status = value["status"].as_str().unwrap_or("?");
            let issues = value["issues"].as_i64().unwrap_or(0);
            let deps = value["deps"].as_i64().unwrap_or(0);
            let comments = value["comments"].as_i64().unwrap_or(0);
            println!("{status}: {issues} issues, {deps} deps, {comments} comments");
        }
    }
}

pub fn print_deleted(mode: OutputMode) {
    match mode {
        OutputMode::Json => print_json(&serde_json::json!({"status": "deleted"})),
        OutputMode::Human => println!("deleted"),
    }
}
