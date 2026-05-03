use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectEntry {
    pub name: String,
    pub path: PathBuf,
    pub remote: Option<String>,
    pub default_branch: Option<String>,
    pub added: String,
    pub updated: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectStore {
    #[serde(default)]
    pub projects: Vec<ProjectEntry>,
}

#[derive(Debug, Clone)]
pub enum AddOutcome {
    Added(ProjectEntry),
    Updated(ProjectEntry),
}

impl ProjectStore {
    pub fn path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".harness")
            .join("projects.json")
    }

    pub fn load() -> Self {
        let path = Self::path();
        if !path.exists() {
            return Self::default();
        }
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn list_sorted(&self) -> Vec<ProjectEntry> {
        let mut projects = self.projects.clone();
        projects.sort_by_key(|a| a.name.to_lowercase());
        projects
    }

    pub fn add(
        &mut self,
        name: Option<String>,
        path: Option<PathBuf>,
        remote: Option<String>,
        default_branch: Option<String>,
    ) -> Result<AddOutcome> {
        let now = chrono::Utc::now().to_rfc3339();
        let project_path = canonicalize_or_absolute(path.unwrap_or(std::env::current_dir()?))?;
        let inferred_name = project_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("project")
            .to_string();
        let project_name = name.unwrap_or(inferred_name);

        if let Some(existing) = self.projects.iter_mut().find(|p| p.path == project_path) {
            existing.name = project_name;
            existing.remote = remote.or_else(|| detect_git_remote(&project_path));
            existing.default_branch = default_branch
                .or_else(|| detect_default_branch(&project_path))
                .or_else(|| existing.default_branch.clone());
            existing.updated = now;
            return Ok(AddOutcome::Updated(existing.clone()));
        }

        if self.projects.iter().any(|p| p.name == project_name) {
            bail!(
                "project name '{}' already exists. Use --name to choose a unique name.",
                project_name
            );
        }

        let entry = ProjectEntry {
            name: project_name,
            path: project_path.clone(),
            remote: remote.or_else(|| detect_git_remote(&project_path)),
            default_branch: default_branch.or_else(|| detect_default_branch(&project_path)),
            added: now.clone(),
            updated: now,
        };
        self.projects.push(entry.clone());
        Ok(AddOutcome::Added(entry))
    }

    pub fn find(&self, target: &str) -> Option<ProjectEntry> {
        if let Some(by_name) = self.projects.iter().find(|p| p.name == target) {
            return Some(by_name.clone());
        }

        let normalized = canonicalize_or_absolute(PathBuf::from(target)).ok()?;
        self.projects.iter().find(|p| p.path == normalized).cloned()
    }

    pub fn remove(&mut self, target: &str) -> Option<ProjectEntry> {
        if let Some(idx) = self.projects.iter().position(|p| p.name == target) {
            return Some(self.projects.remove(idx));
        }

        let normalized = canonicalize_or_absolute(PathBuf::from(target)).ok()?;
        let idx = self.projects.iter().position(|p| p.path == normalized)?;
        Some(self.projects.remove(idx))
    }
}

fn canonicalize_or_absolute(path: PathBuf) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.canonicalize().unwrap_or(path))
    } else {
        let cwd = std::env::current_dir().context("reading current directory")?;
        let abs = cwd.join(path);
        Ok(abs.canonicalize().unwrap_or(abs))
    }
}

fn detect_git_remote(path: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(path)
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn detect_default_branch(path: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(path)
        .args(["symbolic-ref", "--short", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}
