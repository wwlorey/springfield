use serde_json::Value;

use crate::types::FormaError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Json,
    Human,
}

pub fn print_json(value: &Value) {
    println!("{}", serde_json::to_string_pretty(value).unwrap());
}

pub fn print_error(err: &FormaError, mode: OutputMode) {
    match mode {
        OutputMode::Json => {
            eprintln!(
                "{}",
                serde_json::json!({"error": err.to_string(), "code": err.code()})
            );
        }
        OutputMode::Human => {
            eprintln!("error: {err}");
        }
    }
}

pub fn print_spec(value: &Value, mode: OutputMode) {
    match mode {
        OutputMode::Json => print_json(value),
        OutputMode::Human => {
            let stem = value["stem"].as_str().unwrap_or("?");
            let status = value["status"].as_str().unwrap_or("?");
            let purpose = value["purpose"].as_str().unwrap_or("?");
            println!("{stem}  {status}  {purpose}");
        }
    }
}

pub fn print_spec_detail(value: &Value, mode: OutputMode) {
    match mode {
        OutputMode::Json => print_json(value),
        OutputMode::Human => {
            let stem = value["stem"].as_str().unwrap_or("?");
            let purpose = value["purpose"].as_str().unwrap_or("");
            let status = value["status"].as_str().unwrap_or("?");

            println!("# {stem} Specification\n");
            println!("{purpose}\n");
            println!("| Field | Value |");
            println!("|-------|-------|");
            if let Some(src) = value["src"].as_str() {
                println!("| Src | `{src}` |");
            }
            println!("| Status | {status} |");

            if let Some(sections) = value["sections"].as_array() {
                for sec in sections {
                    let name = sec["name"].as_str().unwrap_or("?");
                    let body = sec["body"].as_str().unwrap_or("");
                    println!("\n## {name}\n");
                    println!("{body}");
                }
            }

            if let Some(refs) = value["refs"].as_array()
                && !refs.is_empty()
            {
                println!("\n## Related Specifications\n");
                for r in refs {
                    let rstem = r["stem"].as_str().unwrap_or("?");
                    let rpurpose = r["purpose"].as_str().unwrap_or("");
                    println!("- [{rstem}]({rstem}.md) — {rpurpose}");
                }
            }
        }
    }
}

pub fn print_spec_list(value: &Value, mode: OutputMode) {
    match mode {
        OutputMode::Json => print_json(value),
        OutputMode::Human => {
            if let Some(arr) = value.as_array() {
                if arr.is_empty() {
                    println!("(no specs)");
                } else {
                    for item in arr {
                        print_spec(item, OutputMode::Human);
                    }
                }
            }
        }
    }
}

pub fn print_deleted(stem: &str, mode: OutputMode) {
    match mode {
        OutputMode::Json => print_json(&serde_json::json!({"status": "deleted", "stem": stem})),
        OutputMode::Human => println!("deleted: {stem}"),
    }
}

pub fn print_section(value: &Value, mode: OutputMode) {
    match mode {
        OutputMode::Json => print_json(value),
        OutputMode::Human => {
            let name = value["name"].as_str().unwrap_or("?");
            let slug = value["slug"].as_str().unwrap_or("?");
            let kind = value["kind"].as_str().unwrap_or("?");
            let body = value["body"].as_str().unwrap_or("");
            println!("{slug}  ({kind})  {name}");
            if !body.is_empty() {
                println!("{body}");
            }
        }
    }
}

pub fn print_section_body(value: &Value, mode: OutputMode) {
    match mode {
        OutputMode::Json => print_json(value),
        OutputMode::Human => {
            let body = value["body"].as_str().unwrap_or("");
            print!("{body}");
        }
    }
}

pub fn print_section_list(value: &Value, mode: OutputMode) {
    match mode {
        OutputMode::Json => print_json(value),
        OutputMode::Human => {
            if let Some(arr) = value.as_array() {
                if arr.is_empty() {
                    println!("(no sections)");
                } else {
                    for sec in arr {
                        let pos = sec["position"].as_i64().unwrap_or(0);
                        let slug = sec["slug"].as_str().unwrap_or("?");
                        let kind = sec["kind"].as_str().unwrap_or("?");
                        let body_len = sec["body"].as_str().map(|b| b.len()).unwrap_or(0);
                        println!("{pos}  {slug}  {kind}  ({body_len} bytes)");
                    }
                }
            }
        }
    }
}

pub fn print_section_removed(spec: &str, slug: &str, mode: OutputMode) {
    match mode {
        OutputMode::Json => print_json(&serde_json::json!({
            "status": "removed",
            "spec": spec,
            "slug": slug,
        })),
        OutputMode::Human => println!("removed: {spec}/{slug}"),
    }
}

pub fn print_ref_status(value: &Value, mode: OutputMode) {
    match mode {
        OutputMode::Json => print_json(value),
        OutputMode::Human => {
            let status = value["status"].as_str().unwrap_or("?");
            let from = value["from"].as_str().unwrap_or("?");
            let to = value["to"].as_str().unwrap_or("?");
            println!("ref {status}: {from} -> {to}");
        }
    }
}

pub fn print_ref_list(value: &Value, mode: OutputMode) {
    match mode {
        OutputMode::Json => print_json(value),
        OutputMode::Human => {
            if let Some(arr) = value.as_array() {
                if arr.is_empty() {
                    println!("(no refs)");
                } else {
                    for r in arr {
                        let stem = r["stem"].as_str().unwrap_or("?");
                        let purpose = r["purpose"].as_str().unwrap_or("");
                        println!("{stem}  {purpose}");
                    }
                }
            }
        }
    }
}

pub fn print_ref_tree(value: &Value, mode: OutputMode) {
    match mode {
        OutputMode::Json => print_json(value),
        OutputMode::Human => {
            if let Some(arr) = value.as_array() {
                if arr.is_empty() {
                    println!("(no refs)");
                } else {
                    for node in arr {
                        let depth = node["depth"].as_i64().unwrap_or(0) as usize;
                        let indent = "  ".repeat(depth);
                        let stem = node["stem"].as_str().unwrap_or("?");
                        let purpose = node["purpose"].as_str().unwrap_or("");
                        let status = node["status"].as_str().unwrap_or("?");
                        println!("{indent}{stem} [{status}] {purpose}");
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
                        if let Some(stems) = cycle.as_array() {
                            let chain: Vec<&str> =
                                stems.iter().filter_map(|v| v.as_str()).collect();
                            println!("cycle {}: {}", i + 1, chain.join(" -> "));
                        }
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

pub fn print_count(value: &Value, mode: OutputMode) {
    match mode {
        OutputMode::Json => print_json(value),
        OutputMode::Human => {
            if let Some(count) = value["count"].as_i64() {
                println!("count: {count}");
                if let Some(groups) = value["groups"].as_array() {
                    for g in groups {
                        let status = g["status"].as_str().unwrap_or("?");
                        let count = g["count"].as_i64().unwrap_or(0);
                        println!("  {status}: {count}");
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
            if let Some(obj) = value.as_object() {
                for (key, val) in obj {
                    let count = val.as_i64().unwrap_or(0);
                    println!("{key}: {count}");
                }
            }
        }
    }
}

pub fn print_import_export(value: &Value, mode: OutputMode) {
    match mode {
        OutputMode::Json => print_json(value),
        OutputMode::Human => {
            let status = value["status"].as_str().unwrap_or("?");
            let specs = value["specs"].as_i64().unwrap_or(0);
            let sections = value["sections"].as_i64().unwrap_or(0);
            let refs = value["refs"].as_i64().unwrap_or(0);
            println!("{status}: {specs} specs, {sections} sections, {refs} refs");
        }
    }
}

pub fn print_check(value: &Value, mode: OutputMode) {
    match mode {
        OutputMode::Json => print_json(value),
        OutputMode::Human => {
            let ok = value["ok"].as_bool().unwrap_or(false);
            if let Some(errs) = value["errors"].as_array() {
                for e in errs {
                    let check = e["check"].as_str().unwrap_or("?");
                    let msg = e["message"].as_str().unwrap_or("?");
                    println!("  error [{check}]: {msg}");
                }
            }
            if let Some(warns) = value["warnings"].as_array() {
                for w in warns {
                    let check = w["check"].as_str().unwrap_or("?");
                    let msg = w["message"].as_str().unwrap_or("?");
                    println!("  warn  [{check}]: {msg}");
                }
            }
            if ok {
                println!("check: ok");
            } else {
                println!("check: errors found");
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
                    println!("doctor: no issues found");
                } else {
                    for f in findings {
                        let check = f["check"].as_str().unwrap_or("?");
                        let msg = f["message"].as_str().unwrap_or("?");
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

pub fn print_where(forma_dir: &str, db_dir: &str) {
    println!("jsonl: {forma_dir}");
    println!("db:    {db_dir}");
}
