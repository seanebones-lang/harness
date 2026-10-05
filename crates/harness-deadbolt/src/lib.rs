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
}
