/// Workflow executor — runs declarative YAML multi-step pipelines.
///
/// Workflows live in `.zap/workflows/` (project-local) and are run with:
///   `/run <name>`   — from the REPL
///   `zap --workflow <name>`   — as a CLI flag
///
/// Format:
/// ```yaml
/// name: ship feature
/// description: Review → test → commit → changelog
/// steps:
///   - skill: code-review
///     prompt: "Review staged changes, flag anything blocking"
///     requires_approval: true
///   - skill: test-runner
///     prompt: "Run tests, fix failures autonomously"
///   - prompt: "Commit with a conventional commit message"
/// ```
use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub struct Workflow {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub steps: Vec<WorkflowStep>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkflowStep {
    /// Optional skill to activate for this step (by skill name).
    #[serde(default)]
    pub skill: String,
    /// The prompt to send to the agent for this step.
    pub prompt: String,
    /// If true, pause and ask the user before executing this step.
    #[serde(default)]
    pub requires_approval: bool,
}

// ── Discovery ─────────────────────────────────────────────────────────────────

/// Returns all workflow files in `.zap/workflows/`.
pub fn discover_workflows() -> Vec<(String, PathBuf)> {
    let dir = PathBuf::from(".zap").join("workflows");
    if !dir.is_dir() {
        return Vec::new();
    }
    let mut workflows = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("yaml")
                || path.extension().and_then(|e| e.to_str()) == Some("yml")
            {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    workflows.push((stem.to_string(), path));
                }
            }
        }
    }
    workflows.sort_by(|a, b| a.0.cmp(&b.0));
    workflows
}

/// Load a workflow by name (searches `.zap/workflows/<name>.yaml` and `.yml`).
pub fn load_workflow(name: &str) -> Result<Workflow> {
    let base = PathBuf::from(".zap").join("workflows");
    let yaml_path = base.join(format!("{}.yaml", name));
    let yml_path  = base.join(format!("{}.yml",  name));

    let path = if yaml_path.exists() {
        yaml_path
    } else if yml_path.exists() {
        yml_path
    } else {
        anyhow::bail!(
            "workflow '{}' not found. Create .zap/workflows/{}.yaml",
            name, name
        );
    };

    parse_workflow_file(&path)
}

fn parse_workflow_file(path: &Path) -> Result<Workflow> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("workflow: cannot read {:?}", path))?;
    serde_yaml::from_str(&raw)
        .with_context(|| format!("workflow: cannot parse {:?}", path))
}

// ── Scaffold ──────────────────────────────────────────────────────────────────

/// Create the workflows directory and a sample workflow file.
pub fn scaffold_workflow(name: &str) -> Result<PathBuf> {
    let dir = PathBuf::from(".zap").join("workflows");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.yaml", name));

    if path.exists() {
        anyhow::bail!("workflow '{}' already exists at {:?}", name, path);
    }

    let template = format!(
        r#"name: {name}
description: "Describe what this workflow does"
steps:
  - prompt: "Step 1: describe the task for the agent"
    requires_approval: true

  - skill: test-runner
    prompt: "Step 2: run tests and fix any failures"

  - prompt: "Step 3: summarize what was done"
"#,
        name = name
    );

    std::fs::write(&path, &template)?;
    Ok(path)
}
