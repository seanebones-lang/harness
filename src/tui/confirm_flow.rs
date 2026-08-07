//! Plan-mode confirm helpers (hunk review → ConfirmResult).

use harness_tools::ConfirmResult;

use super::PendingConfirm;
use crate::diff_review::{self, HunkDecision};

/// Accept or reject the current hunk. Returns `Some(result)` when review is complete.
pub(crate) fn decide_hunk(pc: &mut PendingConfirm, accept: bool) -> Option<ConfirmResult> {
    let diff = pc.file_diff.as_mut()?;
    diff_review::set_hunk_decision(diff, pc.hunk_index, accept);
    if let Some(next) = diff_review::next_pending_hunk(diff, pc.hunk_index + 1) {
        pc.hunk_index = next;
        return None;
    }
    Some(finalize_pending(pc))
}

/// Approve all remaining hunks and finalize.
pub(crate) fn approve_all_hunks(pc: &mut PendingConfirm) -> ConfirmResult {
    if let Some(diff) = pc.file_diff.as_mut() {
        for hunk in &mut diff.hunks {
            if hunk.decision == HunkDecision::Pending {
                hunk.decision = HunkDecision::Accept;
            }
        }
    }
    finalize_pending(pc)
}

/// Reject all hunks and deny the tool call.
pub(crate) fn reject_all_hunks(_pc: &PendingConfirm) -> ConfirmResult {
    ConfirmResult::Deny
}

pub(crate) fn finalize_pending(pc: &PendingConfirm) -> ConfirmResult {
    if let Some(diff) = &pc.file_diff {
        match diff_review::finalize_for_apply(diff) {
            None => ConfirmResult::Deny,
            Some((_path, content))
                if content == diff.proposed
                    && diff
                        .hunks
                        .iter()
                        .all(|h| matches!(h.decision, HunkDecision::Accept)) =>
            {
                ConfirmResult::Approve
            }
            Some((path, content)) => ConfirmResult::ApplyContent {
                path: path.to_string_lossy().into(),
                content,
            },
        }
    } else {
        ConfirmResult::Approve
    }
}

pub(crate) fn move_hunk(pc: &mut PendingConfirm, delta: i32) {
    let Some(diff) = &pc.file_diff else { return };
    if diff.hunks.is_empty() {
        return;
    }
    let len = diff.hunks.len() as i32;
    let cur = pc.hunk_index as i32;
    let next = (cur + delta).rem_euclid(len);
    pc.hunk_index = next as usize;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff_review::{DiffHunk, FileDiff, HunkDecision};
    use harness_tools::ConfirmResult;
    use std::path::PathBuf;

    fn hunk(decision: HunkDecision) -> DiffHunk {
        DiffHunk {
            header: "@@".into(),
            lines: vec![('+', "line".into())],
            decision,
        }
    }

    fn file_diff(hunks: Vec<DiffHunk>, proposed: &str) -> FileDiff {
        FileDiff {
            path: PathBuf::from("t.txt"),
            original: Some("old\n".into()),
            proposed: proposed.into(),
            hunks,
            file_decision: None,
        }
    }

    fn pending(diff: Option<FileDiff>, hunk_index: usize) -> PendingConfirm {
        let (tx, _rx) = tokio::sync::oneshot::channel();
        PendingConfirm {
            tool_name: "write_file".into(),
            preview: "preview".into(),
            file_diff: diff,
            hunk_index,
            reply: tx,
        }
    }

    #[test]
    fn finalize_no_diff_approves() {
        let pc = pending(None, 0);
        assert_eq!(finalize_pending(&pc), ConfirmResult::Approve);
    }

    #[test]
    fn finalize_all_reject_denies() {
        let pc = pending(
            Some(file_diff(
                vec![hunk(HunkDecision::Reject), hunk(HunkDecision::Reject)],
                "new\n",
            )),
            0,
        );
        assert_eq!(finalize_pending(&pc), ConfirmResult::Deny);
    }

    #[test]
    fn finalize_all_accept_full_proposed_approves() {
        // Empty hunks → finalize_for_apply returns proposed as-is → Approve path.
        let pc = pending(Some(file_diff(vec![], "full content\n")), 0);
        assert_eq!(finalize_pending(&pc), ConfirmResult::Approve);
    }

    #[test]
    fn reject_all_hunks_always_deny() {
        let pc = pending(Some(file_diff(vec![hunk(HunkDecision::Pending)], "x")), 0);
        assert_eq!(reject_all_hunks(&pc), ConfirmResult::Deny);
    }

    #[test]
    fn approve_all_marks_pending_accept_then_finalizes() {
        let mut pc = pending(
            Some(file_diff(
                vec![hunk(HunkDecision::Pending), hunk(HunkDecision::Reject)],
                "new\n",
            )),
            0,
        );
        let result = approve_all_hunks(&mut pc);
        let diff = pc.file_diff.as_ref().unwrap();
        assert_eq!(diff.hunks[0].decision, HunkDecision::Accept);
        // previously rejected hunk stays rejected
        assert_eq!(diff.hunks[1].decision, HunkDecision::Reject);
        // mixed decisions → ApplyContent (or Approve if reconstruct matches proposed)
        match result {
            ConfirmResult::ApplyContent { path, .. } => assert_eq!(path, "t.txt"),
            ConfirmResult::Approve => {}
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn decide_hunk_advances_then_finalizes() {
        let mut pc = pending(
            Some(file_diff(
                vec![hunk(HunkDecision::Pending), hunk(HunkDecision::Pending)],
                "new\n",
            )),
            0,
        );
        assert!(decide_hunk(&mut pc, true).is_none());
        assert_eq!(pc.hunk_index, 1);
        assert_eq!(
            pc.file_diff.as_ref().unwrap().hunks[0].decision,
            HunkDecision::Accept
        );
        let done = decide_hunk(&mut pc, false);
        assert!(done.is_some());
        assert_eq!(
            pc.file_diff.as_ref().unwrap().hunks[1].decision,
            HunkDecision::Reject
        );
    }

    #[test]
    fn decide_hunk_without_diff_returns_none() {
        let mut pc = pending(None, 0);
        assert!(decide_hunk(&mut pc, true).is_none());
    }

    #[test]
    fn move_hunk_wraps_and_noops_without_diff() {
        let mut pc = pending(
            Some(file_diff(
                vec![
                    hunk(HunkDecision::Pending),
                    hunk(HunkDecision::Pending),
                    hunk(HunkDecision::Pending),
                ],
                "x",
            )),
            0,
        );
        move_hunk(&mut pc, -1);
        assert_eq!(pc.hunk_index, 2);
        move_hunk(&mut pc, 1);
        assert_eq!(pc.hunk_index, 0);
        move_hunk(&mut pc, 5);
        assert_eq!(pc.hunk_index, 2);

        let mut empty = pending(None, 3);
        move_hunk(&mut empty, 1);
        assert_eq!(empty.hunk_index, 3);

        let mut no_hunks = pending(Some(file_diff(vec![], "x")), 1);
        move_hunk(&mut no_hunks, 1);
        assert_eq!(no_hunks.hunk_index, 1);
    }
}
