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
