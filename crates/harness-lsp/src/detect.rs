//! Detect which language server to use for the current project.

use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum LspKind {
    RustAnalyzer,
    TypeScript,
    Pyright,
    Gopls,
}

impl LspKind {
    pub fn binary(&self) -> &'static str {
        match self {
            LspKind::RustAnalyzer => "rust-analyzer",
            LspKind::TypeScript => "typescript-language-server",
            LspKind::Pyright => "pyright-langserver",
            LspKind::Gopls => "gopls",
        }
    }

    pub fn args(&self) -> Vec<&'static str> {
        match self {
            LspKind::TypeScript => vec!["--stdio"],
            LspKind::Pyright => vec!["--stdio"],
            _ => vec![],
        }
    }
}

/// Detect the appropriate language server for a given project root.
/// Returns `None` if no suitable LSP binary is found.
pub fn detect_language_server(root: &Path) -> Option<LspKind> {
    let candidates: &[(LspKind, &str)] = &[
        (LspKind::RustAnalyzer, "Cargo.toml"),
        (LspKind::Gopls, "go.mod"),
        (LspKind::TypeScript, "tsconfig.json"),
        (LspKind::TypeScript, "package.json"),
        (LspKind::Pyright, "pyproject.toml"),
        (LspKind::Pyright, "setup.py"),
    ];

    for (kind, marker) in candidates {
        if root.join(marker).exists() && binary_available(kind.binary()) {
            return Some(kind.clone());
        }
    }

    None
}

fn binary_available(name: &str) -> bool {
    std::process::Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
