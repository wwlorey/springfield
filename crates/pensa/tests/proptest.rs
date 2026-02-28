use pensa::db::Db;
use pensa::types::{CreateIssueParams, IssueType, ListFilters, Priority, Status};
use proptest::prelude::*;
use tempfile::TempDir;

fn open_temp_db() -> (Db, TempDir) {
    let dir = TempDir::new().unwrap();
    let db = Db::open(dir.path()).unwrap();
    (db, dir)
}

fn arb_issue_type() -> impl Strategy<Value = IssueType> {
    prop_oneof![
        Just(IssueType::Bug),
        Just(IssueType::Task),
        Just(IssueType::Test),
        Just(IssueType::Chore),
    ]
}

fn arb_priority() -> impl Strategy<Value = Priority> {
    prop_oneof![
        Just(Priority::P0),
        Just(Priority::P1),
        Just(Priority::P2),
        Just(Priority::P3),
    ]
}

fn arb_status() -> impl Strategy<Value = Status> {
    prop_oneof![
        Just(Status::Open),
        Just(Status::InProgress),
        Just(Status::Closed),
    ]
}

fn arb_title() -> impl Strategy<Value = String> {
    prop_oneof!["[a-zA-Z0-9 _-]{1,80}", "\\PC{1,40}",]
}

fn arb_opt_string() -> impl Strategy<Value = Option<String>> {
    prop_oneof![Just(None), "\\PC{0,60}".prop_map(Some)]
}

fn arb_create_params() -> impl Strategy<Value = CreateIssueParams> {
    (
        arb_title(),
        arb_issue_type(),
        arb_priority(),
        arb_opt_string(),
        arb_opt_string(),
    )
        .prop_map(
            |(title, issue_type, priority, description, spec)| CreateIssueParams {
                title,
                issue_type,
                priority,
                description,
                spec,
                fixes: None,
                assignee: None,
                deps: vec![],
                actor: "prop-agent".into(),
            },
        )
}

/// Edges as index pairs into an issue vec. Self-loops filtered out.
fn arb_edges() -> impl Strategy<Value = (usize, Vec<(usize, usize)>)> {
    (3..10usize).prop_flat_map(|n| {
        let edges = proptest::collection::vec((0..n, 0..n), 0..n * 2).prop_map(|edges| {
            edges
                .into_iter()
                .filter(|(a, b)| a != b)
                .collect::<Vec<_>>()
        });
        (Just(n), edges)
    })
}

fn make_issues(db: &Db, n: usize) -> Vec<String> {
    (0..n)
        .map(|i| {
            db.create_issue(&CreateIssueParams {
                title: format!("node-{i}"),
                issue_type: IssueType::Task,
                priority: Priority::P2,
                description: None,
                spec: None,
                fixes: None,
                assignee: None,
                deps: vec![],
                actor: "prop-agent".into(),
            })
            .unwrap()
            .id
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 1. Dependency graph invariants
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn dep_graph_never_admits_cycle((n, edges) in arb_edges()) {
        let (db, _dir) = open_temp_db();
        let ids = make_issues(&db, n);

        for (child, parent) in &edges {
            let _ = db.add_dep(&ids[*child], &ids[*parent], "prop-agent");
        }

        let cycles = db.detect_cycles().unwrap();
        prop_assert!(cycles.is_empty(), "cycle after add_dep sequence: {:?}", cycles);
    }

    #[test]
    fn ready_never_returns_blocked_issues((n, edges) in arb_edges()) {
        let (db, _dir) = open_temp_db();
        let ids = make_issues(&db, n);

        for (child, parent) in &edges {
            let _ = db.add_dep(&ids[*child], &ids[*parent], "prop-agent");
        }

        let ready: std::collections::HashSet<String> = db
            .ready_issues(&ListFilters::default())
            .unwrap()
            .into_iter()
            .map(|i| i.id)
            .collect();

        let blocked: std::collections::HashSet<String> = db
            .blocked_issues()
            .unwrap()
            .into_iter()
            .map(|i| i.id)
            .collect();

        let overlap: Vec<_> = ready.intersection(&blocked).collect();
        prop_assert!(overlap.is_empty(), "in both ready and blocked: {:?}", overlap);

        for ready_id in &ready {
            let deps = db.list_deps(ready_id).unwrap();
            for dep in &deps {
                prop_assert_eq!(dep.status, Status::Closed,
                    "ready issue {} has open dep {}", ready_id, dep.id);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 2. Export / import roundtrip
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn export_import_preserves_issues(
        params in proptest::collection::vec(arb_create_params(), 1..10)
    ) {
        let (db, _dir) = open_temp_db();

        let mut created_ids = Vec::new();
        for p in &params {
            let issue = db.create_issue(p).unwrap();
            created_ids.push(issue.id);
        }

        for id in created_ids.iter().take(3.min(created_ids.len())) {
            db.add_comment(id, "prop-agent", "test comment \u{1f600}").unwrap();
        }

        let before = db.list_issues(&ListFilters::default()).unwrap();
        let before_comments: Vec<_> = created_ids
            .iter()
            .flat_map(|id| db.list_comments(id).unwrap())
            .collect();

        let export = db.export_jsonl().unwrap();
        prop_assert_eq!(export.issues, before.len());

        db.import_jsonl().unwrap();

        let after = db.list_issues(&ListFilters::default()).unwrap();
        prop_assert_eq!(before.len(), after.len());

        for original in &before {
            let reimported = db.get_issue(&original.id).unwrap();
            prop_assert_eq!(&original.title, &reimported.issue.title);
            prop_assert_eq!(original.issue_type, reimported.issue.issue_type);
            prop_assert_eq!(original.priority, reimported.issue.priority);
            prop_assert_eq!(&original.description, &reimported.issue.description);
            prop_assert_eq!(&original.spec, &reimported.issue.spec);
            prop_assert_eq!(original.status, reimported.issue.status);
            prop_assert_eq!(&original.assignee, &reimported.issue.assignee);
        }

        let after_comments: Vec<_> = created_ids
            .iter()
            .flat_map(|id| db.list_comments(id).unwrap())
            .collect();
        prop_assert_eq!(before_comments.len(), after_comments.len());
        for (bc, ac) in before_comments.iter().zip(after_comments.iter()) {
            prop_assert_eq!(&bc.text, &ac.text);
            prop_assert_eq!(&bc.actor, &ac.actor);
        }
    }
}

// ---------------------------------------------------------------------------
// 3. State machine consistency
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum StateOp {
    Claim(usize, String),
    Release(usize),
    Close(usize),
    Reopen(usize),
}

fn arb_state_scenario() -> impl Strategy<Value = (usize, Vec<StateOp>)> {
    (2..6usize).prop_flat_map(|n| {
        let ops = proptest::collection::vec(
            (0..n, 0..4u8, "[a-z]{3,6}").prop_map(move |(idx, variant, actor)| match variant {
                0 => StateOp::Claim(idx, actor),
                1 => StateOp::Release(idx),
                2 => StateOp::Close(idx),
                _ => StateOp::Reopen(idx),
            }),
            1..20,
        );
        (Just(n), ops)
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn state_machine_invariants_hold((n, ops) in arb_state_scenario()) {
        let (db, _dir) = open_temp_db();
        let ids = make_issues(&db, n);

        for op in &ops {
            match op {
                StateOp::Claim(idx, actor) => { let _ = db.claim_issue(&ids[*idx], actor); }
                StateOp::Release(idx) => { let _ = db.release_issue(&ids[*idx], "prop-agent"); }
                StateOp::Close(idx) => { let _ = db.close_issue(&ids[*idx], None, false, "prop-agent"); }
                StateOp::Reopen(idx) => { let _ = db.reopen_issue(&ids[*idx], None, "prop-agent"); }
            }
        }

        for id in &ids {
            let detail = db.get_issue(id).unwrap();
            let issue = &detail.issue;
            match issue.status {
                Status::InProgress => {
                    prop_assert!(issue.assignee.is_some(),
                        "in_progress issue {} has no assignee", id);
                }
                Status::Closed => {
                    prop_assert!(issue.closed_at.is_some(),
                        "closed issue {} has no closed_at", id);
                }
                Status::Open => {}
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 4. Filter subset property
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn filtered_list_is_subset_of_unfiltered(
        params in proptest::collection::vec(arb_create_params(), 1..15),
        filter_type in proptest::option::of(arb_issue_type()),
        filter_priority in proptest::option::of(arb_priority()),
        filter_status in proptest::option::of(arb_status()),
    ) {
        let (db, _dir) = open_temp_db();
        for p in &params {
            db.create_issue(p).unwrap();
        }

        let all: std::collections::HashSet<String> = db
            .list_issues(&ListFilters::default())
            .unwrap()
            .into_iter()
            .map(|i| i.id)
            .collect();

        let filters = ListFilters {
            issue_type: filter_type,
            priority: filter_priority,
            status: filter_status,
            ..Default::default()
        };

        let filtered_issues = db.list_issues(&filters).unwrap();
        let filtered_ids: std::collections::HashSet<String> =
            filtered_issues.iter().map(|i| i.id.clone()).collect();

        prop_assert!(filtered_ids.is_subset(&all),
            "filtered result contains IDs not in unfiltered: {:?}",
            filtered_ids.difference(&all).collect::<Vec<_>>());

        for issue in &filtered_issues {
            if let Some(ft) = filter_type {
                prop_assert_eq!(issue.issue_type, ft);
            }
            if let Some(fp) = filter_priority {
                prop_assert_eq!(issue.priority, fp);
            }
            if let Some(fs) = filter_status {
                prop_assert_eq!(issue.status, fs);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 5. Enum roundtrip (as_str / FromStr)
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn issue_type_roundtrips(t in arb_issue_type()) {
        let s = t.as_str();
        let parsed: IssueType = s.parse().unwrap();
        prop_assert_eq!(t, parsed);
    }

    #[test]
    fn priority_roundtrips(p in arb_priority()) {
        let s = p.as_str();
        let parsed: Priority = s.parse().unwrap();
        prop_assert_eq!(p, parsed);
    }

    #[test]
    fn status_roundtrips(s in arb_status()) {
        let st = s.as_str();
        let parsed: Status = st.parse().unwrap();
        prop_assert_eq!(s, parsed);
    }
}
