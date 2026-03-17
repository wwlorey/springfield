use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::cursus::toml::{IterDefinition, Mode};

const SENTINEL_MAX_DEPTH: usize = 2;

const SENTINELS: &[&str] = &[".ralph-complete", ".ralph-reject", ".ralph-revise"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IterOutcome {
    Complete,
    Reject,
    Revise,
    Exhausted,
}

impl std::fmt::Display for IterOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Complete => write!(f, "complete"),
            Self::Reject => write!(f, "reject"),
            Self::Revise => write!(f, "revise"),
            Self::Exhausted => write!(f, "exhausted"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NextIter {
    Advance,
    Named(String),
    Stalled,
}

fn find_sentinel(dir: &Path, name: &str, max_depth: usize) -> Option<PathBuf> {
    let candidate = dir.join(name);
    if candidate.exists() {
        return Some(candidate);
    }
    if max_depth == 0 {
        return None;
    }
    let entries = fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        if entry.file_type().ok().is_some_and(|ft| ft.is_dir())
            && let Some(found) = find_sentinel(&entry.path(), name, max_depth - 1)
        {
            return Some(found);
        }
    }
    None
}

pub fn detect_outcome(root: &Path, iter: &IterDefinition) -> IterOutcome {
    if find_sentinel(root, ".ralph-complete", SENTINEL_MAX_DEPTH).is_some() {
        return IterOutcome::Complete;
    }
    if find_sentinel(root, ".ralph-reject", SENTINEL_MAX_DEPTH).is_some() {
        return IterOutcome::Reject;
    }
    if find_sentinel(root, ".ralph-revise", SENTINEL_MAX_DEPTH).is_some() {
        return IterOutcome::Revise;
    }
    if iter.mode == Mode::Interactive && iter.iterations <= 1 {
        return IterOutcome::Complete;
    }
    IterOutcome::Exhausted
}

pub fn clean_sentinels(root: &Path) {
    for name in SENTINELS {
        while let Some(path) = find_sentinel(root, name, SENTINEL_MAX_DEPTH) {
            let _ = fs::remove_file(path);
        }
    }
}

pub fn resolve_transition(iter: &IterDefinition, outcome: &IterOutcome) -> io::Result<NextIter> {
    match outcome {
        IterOutcome::Complete => {
            if let Some(ref target) = iter.next {
                Ok(NextIter::Named(target.clone()))
            } else {
                Ok(NextIter::Advance)
            }
        }
        IterOutcome::Reject => match iter.transitions.on_reject {
            Some(ref target) => Ok(NextIter::Named(target.clone())),
            None => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "iter '{}' signaled reject but no on_reject transition is defined",
                    iter.name
                ),
            )),
        },
        IterOutcome::Revise => match iter.transitions.on_revise {
            Some(ref target) => Ok(NextIter::Named(target.clone())),
            None => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "iter '{}' signaled revise but no on_revise transition is defined",
                    iter.name
                ),
            )),
        },
        IterOutcome::Exhausted => Ok(NextIter::Stalled),
    }
}

pub fn resolve_iter_index(
    iters: &[IterDefinition],
    current_index: usize,
    next: &NextIter,
) -> Option<usize> {
    match next {
        NextIter::Advance => {
            let next_idx = current_index + 1;
            if next_idx < iters.len() {
                Some(next_idx)
            } else {
                None
            }
        }
        NextIter::Named(name) => iters.iter().position(|i| i.name == *name),
        NextIter::Stalled => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cursus::toml::Transitions;
    use tempfile::TempDir;

    fn make_iter(
        name: &str,
        mode: Mode,
        iterations: u32,
        next: Option<&str>,
        on_reject: Option<&str>,
        on_revise: Option<&str>,
    ) -> IterDefinition {
        IterDefinition {
            name: name.to_string(),
            prompt: format!("{name}.md"),
            mode,
            iterations,
            produces: None,
            consumes: vec![],
            auto_push: None,
            next: next.map(|s| s.to_string()),
            transitions: Transitions {
                on_reject: on_reject.map(|s| s.to_string()),
                on_revise: on_revise.map(|s| s.to_string()),
            },
        }
    }

    #[test]
    fn detect_complete_sentinel() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(".ralph-complete"), "").unwrap();
        let iter = make_iter("build", Mode::Afk, 10, None, None, None);
        assert_eq!(detect_outcome(tmp.path(), &iter), IterOutcome::Complete);
    }

    #[test]
    fn detect_reject_sentinel() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(".ralph-reject"), "").unwrap();
        let iter = make_iter("review", Mode::Interactive, 1, None, Some("draft"), None);
        assert_eq!(detect_outcome(tmp.path(), &iter), IterOutcome::Reject);
    }

    #[test]
    fn detect_revise_sentinel() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(".ralph-revise"), "").unwrap();
        let iter = make_iter("review", Mode::Interactive, 1, None, None, Some("revise"));
        assert_eq!(detect_outcome(tmp.path(), &iter), IterOutcome::Revise);
    }

    #[test]
    fn complete_wins_over_reject_and_revise() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(".ralph-complete"), "").unwrap();
        fs::write(tmp.path().join(".ralph-reject"), "").unwrap();
        fs::write(tmp.path().join(".ralph-revise"), "").unwrap();
        let iter = make_iter("review", Mode::Afk, 10, None, Some("draft"), Some("fix"));
        assert_eq!(detect_outcome(tmp.path(), &iter), IterOutcome::Complete);
    }

    #[test]
    fn reject_wins_over_revise() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(".ralph-reject"), "").unwrap();
        fs::write(tmp.path().join(".ralph-revise"), "").unwrap();
        let iter = make_iter("review", Mode::Afk, 10, None, Some("draft"), Some("fix"));
        assert_eq!(detect_outcome(tmp.path(), &iter), IterOutcome::Reject);
    }

    #[test]
    fn interactive_no_sentinel_is_complete() {
        let tmp = TempDir::new().unwrap();
        let iter = make_iter("review", Mode::Interactive, 1, None, None, None);
        assert_eq!(detect_outcome(tmp.path(), &iter), IterOutcome::Complete);
    }

    #[test]
    fn afk_no_sentinel_is_exhausted() {
        let tmp = TempDir::new().unwrap();
        let iter = make_iter("build", Mode::Afk, 10, None, None, None);
        assert_eq!(detect_outcome(tmp.path(), &iter), IterOutcome::Exhausted);
    }

    #[test]
    fn interactive_multi_iteration_no_sentinel_is_exhausted() {
        let tmp = TempDir::new().unwrap();
        let iter = make_iter("review", Mode::Interactive, 5, None, None, None);
        assert_eq!(detect_outcome(tmp.path(), &iter), IterOutcome::Exhausted);
    }

    #[test]
    fn nested_sentinel_detected() {
        let tmp = TempDir::new().unwrap();
        let nested = tmp.path().join("sub");
        fs::create_dir(&nested).unwrap();
        fs::write(nested.join(".ralph-complete"), "").unwrap();
        let iter = make_iter("build", Mode::Afk, 10, None, None, None);
        assert_eq!(detect_outcome(tmp.path(), &iter), IterOutcome::Complete);
    }

    #[test]
    fn sentinel_too_deep_not_detected() {
        let tmp = TempDir::new().unwrap();
        let deep = tmp.path().join("a").join("b").join("c");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join(".ralph-complete"), "").unwrap();
        let iter = make_iter("build", Mode::Interactive, 5, None, None, None);
        assert_eq!(detect_outcome(tmp.path(), &iter), IterOutcome::Exhausted);
    }

    #[test]
    fn clean_sentinels_removes_all() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(".ralph-complete"), "").unwrap();
        fs::write(tmp.path().join(".ralph-reject"), "").unwrap();
        fs::write(tmp.path().join(".ralph-revise"), "").unwrap();
        let nested = tmp.path().join("sub");
        fs::create_dir(&nested).unwrap();
        fs::write(nested.join(".ralph-complete"), "").unwrap();

        clean_sentinels(tmp.path());

        assert!(!tmp.path().join(".ralph-complete").exists());
        assert!(!tmp.path().join(".ralph-reject").exists());
        assert!(!tmp.path().join(".ralph-revise").exists());
        assert!(!nested.join(".ralph-complete").exists());
    }

    #[test]
    fn clean_sentinels_noop_when_none() {
        let tmp = TempDir::new().unwrap();
        clean_sentinels(tmp.path());
    }

    #[test]
    fn resolve_complete_advances() {
        let iter = make_iter("build", Mode::Afk, 10, None, None, None);
        let next = resolve_transition(&iter, &IterOutcome::Complete).unwrap();
        assert_eq!(next, NextIter::Advance);
    }

    #[test]
    fn resolve_complete_with_next_override() {
        let iter = make_iter("revise", Mode::Afk, 5, Some("review"), None, None);
        let next = resolve_transition(&iter, &IterOutcome::Complete).unwrap();
        assert_eq!(next, NextIter::Named("review".to_string()));
    }

    #[test]
    fn resolve_reject_follows_on_reject() {
        let iter = make_iter("review", Mode::Interactive, 1, None, Some("draft"), None);
        let next = resolve_transition(&iter, &IterOutcome::Reject).unwrap();
        assert_eq!(next, NextIter::Named("draft".to_string()));
    }

    #[test]
    fn resolve_reject_without_transition_errors() {
        let iter = make_iter("review", Mode::Interactive, 1, None, None, None);
        let err = resolve_transition(&iter, &IterOutcome::Reject).unwrap_err();
        assert!(
            err.to_string()
                .contains("iter 'review' signaled reject but no on_reject transition is defined")
        );
    }

    #[test]
    fn resolve_revise_follows_on_revise() {
        let iter = make_iter("review", Mode::Interactive, 1, None, None, Some("revise"));
        let next = resolve_transition(&iter, &IterOutcome::Revise).unwrap();
        assert_eq!(next, NextIter::Named("revise".to_string()));
    }

    #[test]
    fn resolve_revise_without_transition_errors() {
        let iter = make_iter("review", Mode::Interactive, 1, None, None, None);
        let err = resolve_transition(&iter, &IterOutcome::Revise).unwrap_err();
        assert!(
            err.to_string()
                .contains("iter 'review' signaled revise but no on_revise transition is defined")
        );
    }

    #[test]
    fn resolve_exhausted_stalls() {
        let iter = make_iter("build", Mode::Afk, 10, None, None, None);
        let next = resolve_transition(&iter, &IterOutcome::Exhausted).unwrap();
        assert_eq!(next, NextIter::Stalled);
    }

    #[test]
    fn resolve_iter_index_advance() {
        let iters = vec![
            make_iter("a", Mode::Afk, 1, None, None, None),
            make_iter("b", Mode::Afk, 1, None, None, None),
            make_iter("c", Mode::Afk, 1, None, None, None),
        ];
        assert_eq!(resolve_iter_index(&iters, 0, &NextIter::Advance), Some(1));
        assert_eq!(resolve_iter_index(&iters, 1, &NextIter::Advance), Some(2));
        assert_eq!(resolve_iter_index(&iters, 2, &NextIter::Advance), None);
    }

    #[test]
    fn resolve_iter_index_named() {
        let iters = vec![
            make_iter("draft", Mode::Afk, 1, None, None, None),
            make_iter("review", Mode::Interactive, 1, None, None, None),
            make_iter("approve", Mode::Interactive, 1, None, None, None),
        ];
        assert_eq!(
            resolve_iter_index(&iters, 1, &NextIter::Named("draft".to_string())),
            Some(0)
        );
        assert_eq!(
            resolve_iter_index(&iters, 0, &NextIter::Named("approve".to_string())),
            Some(2)
        );
        assert_eq!(
            resolve_iter_index(&iters, 0, &NextIter::Named("nonexistent".to_string())),
            None
        );
    }

    #[test]
    fn resolve_iter_index_stalled() {
        let iters = vec![make_iter("a", Mode::Afk, 1, None, None, None)];
        assert_eq!(resolve_iter_index(&iters, 0, &NextIter::Stalled), None);
    }

    #[test]
    fn back_edge_transition() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(".ralph-reject"), "").unwrap();

        let iters = vec![
            make_iter("draft", Mode::Afk, 10, None, None, None),
            make_iter("review", Mode::Interactive, 1, None, Some("draft"), None),
        ];

        let outcome = detect_outcome(tmp.path(), &iters[1]);
        assert_eq!(outcome, IterOutcome::Reject);

        let next = resolve_transition(&iters[1], &outcome).unwrap();
        assert_eq!(next, NextIter::Named("draft".to_string()));

        let idx = resolve_iter_index(&iters, 1, &next);
        assert_eq!(idx, Some(0));
    }

    #[test]
    fn final_iter_complete_returns_none() {
        let iters = vec![
            make_iter("build", Mode::Afk, 10, None, None, None),
            make_iter("approve", Mode::Interactive, 1, None, None, None),
        ];
        let next = resolve_transition(&iters[1], &IterOutcome::Complete).unwrap();
        assert_eq!(next, NextIter::Advance);
        assert_eq!(resolve_iter_index(&iters, 1, &next), None);
    }

    #[test]
    fn outcome_display() {
        assert_eq!(IterOutcome::Complete.to_string(), "complete");
        assert_eq!(IterOutcome::Reject.to_string(), "reject");
        assert_eq!(IterOutcome::Revise.to_string(), "revise");
        assert_eq!(IterOutcome::Exhausted.to_string(), "exhausted");
    }
}
