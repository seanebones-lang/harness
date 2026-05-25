#[cfg(test)]
mod error_handling_tests {
    use anyhow::Result;
    use harness_memory::SessionStore;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn test_session_store_open_failure() -> Result<()> {
        // Test opening SessionStore where the parent path is a file (not a dir).
        // create_dir_all will fail trying to create a directory where a file exists.
        let dir = tempdir()?;
        let file_as_parent = dir.path().join("existing_file");
        std::fs::write(&file_as_parent, b"not a directory")?;
        let invalid_path = file_as_parent.join("db.db");
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
