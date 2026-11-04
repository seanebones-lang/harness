//! Thin re-export. The gate lives in the Apache-2.0 `deadbolt` crate.
//! This wrapper does not relicense Harness.

#[cfg(feature = "deadbolt")]
pub use deadbolt_dep::*;

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(feature = "deadbolt")]
    fn reexport_keeps_the_gate() {
        assert!(crate::is_shutdown_tool("deadbolt"));
        assert!(!crate::is_shutdown_tool("shell"));
    }

    #[test]
    #[cfg(feature = "deadbolt")]
    fn activity_compatibility_does_not_replace_supervision_or_deadline() {
        let root = std::env::temp_dir().join(format!(
            "harness-deadbolt-mode-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        {
            let gate = crate::Deadbolt::open_at(&root, true, 60);
            let deadline = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64
                + 300;
            gate.issue_agent("supervised", deadline).unwrap();
            let before = gate.status(Some("supervised")).unwrap().remove(0);
            gate.ensure_activity_agent("supervised").unwrap();
            let after = gate.status(Some("supervised")).unwrap().remove(0);
            assert_eq!(after.supervision_root.as_deref(), Some("supervised"));
            assert_eq!(after.hard_deadline_at, Some(deadline));
            assert_eq!(after.expires_at, before.expires_at);
            gate.force_expire("supervised").unwrap();
            gate.ensure_activity_agent("supervised").unwrap();
            assert!(matches!(
                gate.admit("supervised", "shell"),
                crate::AdmitDecision::Deny { .. }
            ));
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
