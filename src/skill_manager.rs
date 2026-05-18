/// Skill system — lazy loading of markdown skill files.
///
/// Skills live in:
///   - `~/.zap/skills/`    (global, shared across all projects)
///   - `.zap/skills/`      (project-local, checked into the repo)
///
/// Format: YAML frontmatter + markdown body:
/// ```markdown
/// ---
/// name: conventional-commits
/// trigger: ["commit", "git log", "stage", "push", "changelog"]
/// tokens: 800
/// extends: []
/// ---
/// When committing code, always use the Conventional Commits format:
/// `<type>(<scope>): <description>`
/// Types: feat, fix, docs, style, refactor, perf, test, chore
/// ```
use anyhow::Result;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Skill {
    pub name:           String,
    pub description:    String,
    pub license:        Option<String>,
    pub triggers:       Vec<String>,
    pub token_estimate: usize,
    pub content:        String,
    pub source:         SkillSource,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SkillSource {
    Bundled,  // shipped with the binary, lowest priority
    Global,   // ~/.zap/skills/
    Project,  // .zap/skills/  — highest priority
}

impl Skill {
    /// Skills with no triggers are injected on every turn (meta-guidance).
    pub fn is_always_on(&self) -> bool {
        self.triggers.is_empty()
    }

    /// Returns true if any trigger keyword appears in the query (case-insensitive).
    pub fn matches(&self, query: &str) -> bool {
        let lower = query.to_lowercase();
        self.triggers.iter().any(|t| lower.contains(t.as_str()))
    }

    /// Approximate token count (chars / 4).
    pub fn tokens(&self) -> usize {
        if self.token_estimate > 0 {
            self.token_estimate
        } else {
            self.content.len() / 4
        }
    }
}

/// Short display label for a skill's source tier.
pub fn source_label(source: &SkillSource) -> &'static str {
    match source {
        SkillSource::Bundled => "built-in",
        SkillSource::Global  => "global",
        SkillSource::Project => "project",
    }
}

// ── Discovery ─────────────────────────────────────────────────────────────────

/// Returns all skill directories to scan, in priority order (project overrides global).
fn skill_dirs() -> Vec<(PathBuf, SkillSource)> {
    let mut dirs = Vec::new();

    // Global: ~/.zap/skills/
    if let Some(home) = dirs::home_dir() {
        let global = home.join(".zap").join("skills");
        if global.is_dir() {
            dirs.push((global, SkillSource::Global));
        }
    }

    // Project-local: .zap/skills/ (relative to cwd)
    let local = PathBuf::from(".zap").join("skills");
    if local.is_dir() {
        dirs.push((local, SkillSource::Project));
    }

    dirs
}

/// Load all skills: bundled defaults first, then global (~/.zap/skills/),
/// then project-local (.zap/skills/). Higher-priority skills override bundled
/// ones of the same name.
pub fn load_all_skills() -> Vec<Skill> {
    // Start with bundled defaults.
    let mut skills: Vec<Skill> = bundled_skills();

    // Overlay user skills (global then project). Same-name skills replace bundled/global.
    for (dir, source) in skill_dirs() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("md") {
                    match parse_skill_file(&path, source.clone()) {
                        Ok(skill) => {
                            // Replace any existing skill with the same name.
                            if let Some(pos) = skills.iter().position(|s| s.name == skill.name) {
                                skills[pos] = skill;
                            } else {
                                skills.push(skill);
                            }
                        }
                        Err(e) => tracing::warn!("skill: could not parse {:?}: {}", path, e),
                    }
                }
            }
        }
    }

    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

/// Match skills to a user query. Returns all matching skills, de-duplicated by name.
/// Project-local skills take precedence over global skills of the same name.
pub fn match_skills<'a>(query: &str, skills: &'a [Skill]) -> Vec<&'a Skill> {
    let mut matched: Vec<&Skill> = Vec::new();
    for skill in skills {
        if skill.matches(query) {
            // De-duplicate: if we already have a skill with the same name, keep the
            // project-local one (which comes later in the sorted list).
            if let Some(existing) = matched.iter_mut().find(|s| s.name == skill.name) {
                if skill.source == SkillSource::Project {
                    *existing = skill;
                }
            } else {
                matched.push(skill);
            }
        }
    }
    matched
}

/// Build the skill injection string for the system prompt.
pub fn build_skill_prompt(skills: &[&Skill]) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let mut parts = vec!["## Active Skills".to_string()];
    for skill in skills {
        parts.push(format!("### {} (~{}t)\n{}", skill.name, skill.tokens(), skill.content.trim()));
    }
    parts.join("\n\n")
}

/// Summary line for display in the turn header.
pub fn skills_summary(skills: &[&Skill]) -> String {
    if skills.is_empty() {
        return String::new();
    }
    skills
        .iter()
        .map(|s| format!("{} (~{}t)", s.name, s.tokens()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Skills with no triggers — injected once into the base system prompt at session start.
pub fn always_on_skills(skills: &[Skill]) -> Vec<&Skill> {
    skills.iter().filter(|s| s.is_always_on()).collect()
}

/// Build the always-on block that gets baked into the base system prompt.
pub fn build_always_on_prompt(skills: &[&Skill]) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let mut parts = vec!["## Always-Active Guidelines".to_string()];
    for skill in skills {
        parts.push(format!("### {}\n{}", skill.name, skill.content.trim()));
    }
    parts.join("\n\n")
}

// ── Parsing ───────────────────────────────────────────────────────────────────

fn parse_skill_file(path: &std::path::Path, source: SkillSource) -> Result<Skill> {
    let raw = std::fs::read_to_string(path)?;
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    if let Some(content_after_fence) = raw.strip_prefix("---") {
        if let Some(end_idx) = content_after_fence.find("\n---") {
            let fm   = parse_frontmatter(&content_after_fence[..end_idx]);
            let body = content_after_fence[end_idx + 4..].trim_start().to_string();
            return Ok(Skill {
                name,
                description:    fm.description,
                license:        fm.license,
                triggers:       fm.triggers,
                token_estimate: fm.tokens,
                content:        body,
                source,
            });
        }
    }

    // No frontmatter — treat entire file as skill body, always-on (no triggers).
    Ok(Skill {
        name,
        description:    String::new(),
        license:        None,
        triggers:       Vec::new(),
        token_estimate: 0,
        content:        raw,
        source,
    })
}

struct ParsedFrontmatter {
    description: String,
    license:     Option<String>,
    triggers:    Vec<String>,
    tokens:      usize,
}

fn parse_frontmatter(fm: &str) -> ParsedFrontmatter {
    let mut description = String::new();
    let mut license     = None;
    let mut triggers    = Vec::new();
    let mut tokens      = 0usize;

    for line in fm.lines() {
        let line = line.trim();
        if line.starts_with("description:") {
            description = line.trim_start_matches("description:")
                .trim().trim_matches('"').trim_matches('\'').to_string();
        } else if line.starts_with("license:") {
            let lic = line.trim_start_matches("license:")
                .trim().trim_matches('"').trim_matches('\'').to_string();
            if !lic.is_empty() { license = Some(lic); }
        } else if line.starts_with("trigger:") {
            let val = line.trim_start_matches("trigger:").trim();
            let inner = val.trim_matches(|c| c == '[' || c == ']');
            for part in inner.split(',') {
                let t = part.trim().trim_matches('"').trim_matches('\'').to_string();
                if !t.is_empty() { triggers.push(t); }
            }
        } else if line.starts_with("tokens:") {
            let val = line.trim_start_matches("tokens:").trim();
            let clean: String = val.chars().filter(|c| c.is_ascii_digit()).collect();
            tokens = clean.parse().unwrap_or(0);
        }
    }

    ParsedFrontmatter { description, license, triggers, tokens }
}

// ── Bundled default skills ────────────────────────────────────────────────────

/// Skills shipped with the binary. User skills of the same name override these.
fn bundled_skills() -> Vec<Skill> {
    const BUNDLED: &[(&str, &str)] = &[
        // Always-on meta-skills (no triggers — injected every session)
        ("karpathy-guidelines", include_str!("default_skills/karpathy-guidelines.md")),
        // Language skills (trigger on stack detection or keywords)
        ("rust",         include_str!("default_skills/rust.md")),
        ("react",        include_str!("default_skills/react.md")),
        ("typescript",   include_str!("default_skills/typescript.md")),
        ("python",       include_str!("default_skills/python.md")),
        ("go",           include_str!("default_skills/go.md")),
        // Practice skills (trigger on task keywords)
        ("git",          include_str!("default_skills/git.md")),
        ("code-review",  include_str!("default_skills/code-review.md")),
        ("debugging",    include_str!("default_skills/debugging.md")),
        ("security",     include_str!("default_skills/security.md")),
    ];

    BUNDLED.iter().filter_map(|(name, raw)| {
        if let Some(after_fence) = raw.strip_prefix("---") {
            if let Some(end_idx) = after_fence.find("\n---") {
                let fm   = parse_frontmatter(&after_fence[..end_idx]);
                let body = after_fence[end_idx + 4..].trim_start().to_string();
                return Some(Skill {
                    name:           name.to_string(),
                    description:    fm.description,
                    license:        fm.license,
                    triggers:       fm.triggers,
                    token_estimate: fm.tokens,
                    content:        body,
                    source:         SkillSource::Bundled,
                });
            }
        }
        None
    }).collect()
}

// ── Stack auto-detection ──────────────────────────────────────────────────────

/// Detect the project's tech stack from well-known manifest files and return
/// any loaded skills whose name matches a detected stack.
pub fn detect_stack_skills<'a>(skills: &'a [Skill]) -> Vec<&'a Skill> {
    let cwd = std::env::current_dir().unwrap_or_default();

    // Map manifest filename → candidate skill names (first match wins per stack)
    let stacks: &[(&str, &[&str])] = &[
        ("Cargo.toml",                   &["rust"]),
        ("go.mod",                        &["go"]),
        ("package.json",                  &["typescript", "node", "javascript"]),
        ("pyproject.toml",                &["python"]),
        ("setup.py",                      &["python"]),
        ("pom.xml",                       &["java"]),
        ("build.gradle",                  &["java"]),
    ];

    let mut detected_names: Vec<&str> = Vec::new();
    for (manifest, candidates) in stacks {
        if cwd.join(manifest).exists() {
            for &candidate in *candidates {
                if skills.iter().any(|s| s.name == candidate) {
                    detected_names.push(candidate);
                    break; // one skill per stack
                }
            }
        }
    }

    skills.iter()
        .filter(|s| detected_names.contains(&s.name.as_str()))
        .collect()
}

/// Save a skill captured from conversation.
pub fn save_captured_skill(name: &str, content: &str, global: bool) -> Result<PathBuf> {
    let dir = if global {
        dirs::home_dir()
            .map(|h| h.join(".zap").join("skills"))
            .ok_or_else(|| anyhow::anyhow!("cannot locate home directory"))?
    } else {
        PathBuf::from(".zap").join("skills")
    };
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.md", name));
    std::fs::write(&path, content)?;
    Ok(path)
}

// ── Scaffold ──────────────────────────────────────────────────────────────────

/// Create a new skill file from template. Returns the path created.
pub fn create_skill(name: &str, project_local: bool) -> Result<PathBuf> {
    let dir = if project_local {
        PathBuf::from(".zap").join("skills")
    } else {
        dirs::home_dir()
            .map(|h| h.join(".zap").join("skills"))
            .ok_or_else(|| anyhow::anyhow!("cannot locate home directory"))?
    };

    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.md", name));

    if path.exists() {
        anyhow::bail!("skill '{}' already exists at {:?}", name, path);
    }

    let template = format!(
        r#"---
name: {name}
trigger: ["keyword1", "keyword2"]
tokens: 500
---

# {name}

<!-- Describe what this skill does and when it should activate. -->
<!-- These instructions are injected into the agent's context when triggered. -->

When working on tasks related to [topic], follow these guidelines:

1. First guideline
2. Second guideline
3. Third guideline
"#,
        name = name
    );

    std::fs::write(&path, template)?;
    Ok(path)
}
