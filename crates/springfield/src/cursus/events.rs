use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct IterSummary {
    pub name: String,
    pub mode: String,
    pub iterations: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    RunStart {
        run_id: String,
        cursus: String,
        iters: Vec<IterSummary>,
    },
    IterStart {
        iter: String,
        mode: String,
        iteration: u32,
        session_id: String,
    },
    Turn {
        content: String,
        waiting_for_input: bool,
        session_id: String,
    },
    IterComplete {
        iter: String,
        outcome: String,
        iterations_used: u32,
    },
    Transition {
        from_iter: String,
        to_iter: String,
        reason: String,
    },
    ContextProduced {
        key: String,
        iter: String,
    },
    ContextConsumed {
        key: String,
        from_iter: String,
    },
    Stall {
        iter: String,
        iterations_attempted: u32,
        actions: Vec<String>,
    },
    Retry {
        attempt: u32,
        reason: String,
        next_retry_secs: u64,
    },
    RunComplete {
        status: String,
        run_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        resume_command: Option<String>,
    },
    Error {
        message: String,
        fatal: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        iter: Option<String>,
    },
}

pub fn emit_event(event: &Event) {
    if let Ok(json) = serde_json::to_string(event) {
        println!("{json}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_event(event: &Event) -> serde_json::Value {
        let json = serde_json::to_string(event).unwrap();
        serde_json::from_str(&json).unwrap()
    }

    #[test]
    fn run_start_serializes_with_event_field() {
        let event = Event::RunStart {
            run_id: "change-20260422T150000".into(),
            cursus: "change".into(),
            iters: vec![IterSummary {
                name: "change".into(),
                mode: "interactive".into(),
                iterations: 1,
            }],
        };
        let v = parse_event(&event);
        assert_eq!(v["event"], "run_start");
        assert_eq!(v["run_id"], "change-20260422T150000");
        assert_eq!(v["cursus"], "change");
        assert_eq!(v["iters"][0]["name"], "change");
        assert_eq!(v["iters"][0]["mode"], "interactive");
        assert_eq!(v["iters"][0]["iterations"], 1);
    }

    #[test]
    fn iter_start_serializes_with_event_field() {
        let event = Event::IterStart {
            iter: "change".into(),
            mode: "interactive".into(),
            iteration: 1,
            session_id: "a1b2c3d4-e5f6-7890-abcd-ef1234567890".into(),
        };
        let v = parse_event(&event);
        assert_eq!(v["event"], "iter_start");
        assert_eq!(v["iter"], "change");
        assert_eq!(v["mode"], "interactive");
        assert_eq!(v["iteration"], 1);
        assert_eq!(v["session_id"], "a1b2c3d4-e5f6-7890-abcd-ef1234567890");
    }

    #[test]
    fn turn_serializes_with_waiting_for_input() {
        let event = Event::Turn {
            content: "Should I use bcrypt?".into(),
            waiting_for_input: true,
            session_id: "sess-1".into(),
        };
        let v = parse_event(&event);
        assert_eq!(v["event"], "turn");
        assert_eq!(v["content"], "Should I use bcrypt?");
        assert_eq!(v["waiting_for_input"], true);
        assert_eq!(v["session_id"], "sess-1");
    }

    #[test]
    fn turn_serializes_not_waiting() {
        let event = Event::Turn {
            content: "Done.".into(),
            waiting_for_input: false,
            session_id: "sess-1".into(),
        };
        let v = parse_event(&event);
        assert_eq!(v["waiting_for_input"], false);
    }

    #[test]
    fn iter_complete_serializes_with_event_field() {
        let event = Event::IterComplete {
            iter: "change".into(),
            outcome: "complete".into(),
            iterations_used: 1,
        };
        let v = parse_event(&event);
        assert_eq!(v["event"], "iter_complete");
        assert_eq!(v["iter"], "change");
        assert_eq!(v["outcome"], "complete");
        assert_eq!(v["iterations_used"], 1);
    }

    #[test]
    fn iter_complete_exhausted_outcome() {
        let event = Event::IterComplete {
            iter: "build".into(),
            outcome: "exhausted".into(),
            iterations_used: 10,
        };
        let v = parse_event(&event);
        assert_eq!(v["outcome"], "exhausted");
        assert_eq!(v["iterations_used"], 10);
    }

    #[test]
    fn transition_serializes_with_event_field() {
        let event = Event::Transition {
            from_iter: "review".into(),
            to_iter: "revise".into(),
            reason: "revise".into(),
        };
        let v = parse_event(&event);
        assert_eq!(v["event"], "transition");
        assert_eq!(v["from_iter"], "review");
        assert_eq!(v["to_iter"], "revise");
        assert_eq!(v["reason"], "revise");
    }

    #[test]
    fn context_produced_serializes_with_event_field() {
        let event = Event::ContextProduced {
            key: "discuss-summary".into(),
            iter: "discuss".into(),
        };
        let v = parse_event(&event);
        assert_eq!(v["event"], "context_produced");
        assert_eq!(v["key"], "discuss-summary");
        assert_eq!(v["iter"], "discuss");
    }

    #[test]
    fn context_consumed_serializes_with_event_field() {
        let event = Event::ContextConsumed {
            key: "discuss-summary".into(),
            from_iter: "discuss".into(),
        };
        let v = parse_event(&event);
        assert_eq!(v["event"], "context_consumed");
        assert_eq!(v["key"], "discuss-summary");
        assert_eq!(v["from_iter"], "discuss");
    }

    #[test]
    fn stall_serializes_with_actions() {
        let event = Event::Stall {
            iter: "implement".into(),
            iterations_attempted: 5,
            actions: vec!["retry".into(), "skip".into(), "abort".into()],
        };
        let v = parse_event(&event);
        assert_eq!(v["event"], "stall");
        assert_eq!(v["iter"], "implement");
        assert_eq!(v["iterations_attempted"], 5);
        let actions = v["actions"].as_array().unwrap();
        assert_eq!(actions.len(), 3);
        assert_eq!(actions[0], "retry");
        assert_eq!(actions[1], "skip");
        assert_eq!(actions[2], "abort");
    }

    #[test]
    fn retry_serializes_with_event_field() {
        let event = Event::Retry {
            attempt: 4,
            reason: "process_crash".into(),
            next_retry_secs: 300,
        };
        let v = parse_event(&event);
        assert_eq!(v["event"], "retry");
        assert_eq!(v["attempt"], 4);
        assert_eq!(v["reason"], "process_crash");
        assert_eq!(v["next_retry_secs"], 300);
    }

    #[test]
    fn run_complete_serializes_with_event_field() {
        let event = Event::RunComplete {
            status: "completed".into(),
            run_id: "change-20260422T150000".into(),
            resume_command: None,
        };
        let v = parse_event(&event);
        assert_eq!(v["event"], "run_complete");
        assert_eq!(v["status"], "completed");
        assert_eq!(v["run_id"], "change-20260422T150000");
        assert!(
            v.get("resume_command").is_none(),
            "completed run_complete should not include resume_command"
        );
    }

    #[test]
    fn run_complete_stalled_status() {
        let event = Event::RunComplete {
            status: "stalled".into(),
            run_id: "build-20260422T150000".into(),
            resume_command: Some("sgf build --resume build-20260422T150000".into()),
        };
        let v = parse_event(&event);
        assert_eq!(v["status"], "stalled");
        assert_eq!(
            v["resume_command"],
            "sgf build --resume build-20260422T150000"
        );
    }

    #[test]
    fn error_serializes_with_event_field() {
        let event = Event::Error {
            message: "prompt not found: change.md".into(),
            fatal: true,
            iter: Some("change".into()),
        };
        let v = parse_event(&event);
        assert_eq!(v["event"], "error");
        assert_eq!(v["message"], "prompt not found: change.md");
        assert_eq!(v["fatal"], true);
        assert_eq!(v["iter"], "change");
    }

    #[test]
    fn error_without_iter_omits_field() {
        let event = Event::Error {
            message: "failed to create run directory".into(),
            fatal: true,
            iter: None,
        };
        let v = parse_event(&event);
        assert_eq!(v["event"], "error");
        assert!(v.get("iter").is_none());
    }

    #[test]
    fn all_events_produce_valid_json_lines() {
        let events: Vec<Event> = vec![
            Event::RunStart {
                run_id: "r1".into(),
                cursus: "c1".into(),
                iters: vec![],
            },
            Event::IterStart {
                iter: "i1".into(),
                mode: "afk".into(),
                iteration: 1,
                session_id: "s1".into(),
            },
            Event::Turn {
                content: "hello".into(),
                waiting_for_input: true,
                session_id: "s1".into(),
            },
            Event::IterComplete {
                iter: "i1".into(),
                outcome: "complete".into(),
                iterations_used: 1,
            },
            Event::Transition {
                from_iter: "a".into(),
                to_iter: "b".into(),
                reason: "complete".into(),
            },
            Event::ContextProduced {
                key: "k".into(),
                iter: "i".into(),
            },
            Event::ContextConsumed {
                key: "k".into(),
                from_iter: "i".into(),
            },
            Event::Stall {
                iter: "i".into(),
                iterations_attempted: 5,
                actions: vec!["retry".into()],
            },
            Event::Retry {
                attempt: 1,
                reason: "crash".into(),
                next_retry_secs: 60,
            },
            Event::RunComplete {
                status: "completed".into(),
                run_id: "r1".into(),
                resume_command: None,
            },
            Event::Error {
                message: "oops".into(),
                fatal: false,
                iter: None,
            },
        ];

        for event in &events {
            let json = serde_json::to_string(event).unwrap();
            assert!(!json.contains('\n'), "JSON line must not contain newlines");
            let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
            assert!(
                parsed.get("event").is_some(),
                "every event must have an 'event' field"
            );
        }
    }

    #[test]
    fn run_start_with_multiple_iters() {
        let event = Event::RunStart {
            run_id: "spec-20260422T150000".into(),
            cursus: "spec".into(),
            iters: vec![
                IterSummary {
                    name: "discuss".into(),
                    mode: "interactive".into(),
                    iterations: 1,
                },
                IterSummary {
                    name: "draft".into(),
                    mode: "afk".into(),
                    iterations: 10,
                },
                IterSummary {
                    name: "review".into(),
                    mode: "interactive".into(),
                    iterations: 1,
                },
            ],
        };
        let v = parse_event(&event);
        assert_eq!(v["iters"].as_array().unwrap().len(), 3);
        assert_eq!(v["iters"][1]["name"], "draft");
        assert_eq!(v["iters"][1]["mode"], "afk");
        assert_eq!(v["iters"][1]["iterations"], 10);
    }
}
