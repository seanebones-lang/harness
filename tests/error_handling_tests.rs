#[cfg(test)]
mod error_handling_tests {
    use anyhow::Result;
    use harness_memory::SessionStore;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn test_session_store_open_failure() -> Result<()> {
        // Test opening SessionStore with an invalid path
        let invalid_path = PathBuf::from("/invalid/path/to/db.db");
        let result = SessionStore::open(&invalid_path);
        assert!(result.is_err(), "Opening with invalid path should fail");
        Ok(())
    }

    #[test]
    fn test_session_store_basic_operations() -> Result<()> {
        let dir = tempdir()?;
        let db_path = dir.path().join("test.db");
        let _store = SessionStore::open(&db_path)?;
        // Add more test cases for save/load/find operations
        Ok(())
    }
}
