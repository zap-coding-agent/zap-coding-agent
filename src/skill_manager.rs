/// Skill system — lazy loading of markdown skill files.
///
/// Skills live in:
///   - `~/.zap/skills/`    (global, shared across all projects)
///   - `.zap/skills/`      (project-local, checked into the repo)
///
/// Three categories control when a skill is active:
///   - Core     — injected into every session's system prompt (no trigger needed)
///   - Practice — always a trigger candidate; useful across any stack (git, debugging…)
///   - Domain   — session-scoped; only trigger-matchable after startup.
///     Auto-detected from manifests, or selected via the startup prompt.
///
/// Frontmatter format:
/// ```markdown
/// ---
/// name: rust
/// category: domain
/// trigger: ["rust", "cargo", "fn "]
/// tokens: ~700
/// ---
/// ```
use anyhow::Result;
use std::path::PathBuf;

/// Check that `trigger` (already lowercased) appears in `text` (already lowercased)
/// at word boundaries. Strategy depends on trigger length to balance false positives
/// against useful inflected matches:
///
/// - Triggers that don't start with an ASCII letter (e.g. ".rs", "#include", "fn "):
///   simple substring — these are inherently anchor-like so boundaries don't apply.
/// - Short alphabetic triggers (≤ 3 chars, e.g. "pr", "go", "sql"):
///   require BOTH word boundaries so "pr" doesn't match "process".
/// - Longer alphabetic triggers (≥ 4 chars, e.g. "review"):
///   require only a word-START boundary so "reviewing" still matches "review"
///   but "preview" and "irrelevant" do not.
fn trigger_word_match(text: &str, trigger: &str) -> bool {
    let tb = trigger.as_bytes();
    let qb = text.as_bytes();
    let tlen = tb.len();
    if tlen == 0 { return false; }

    let first_alpha = trigger.starts_with(|c: char| c.is_ascii_alphabetic());
    let require_post = first_alpha && tlen <= 3;

    let mut i = 0;
    while i + tlen <= qb.len() {
        if &qb[i..i + tlen] == tb {
            let pre_ok = i == 0 || !qb[i - 1].is_ascii_alphabetic();
            if pre_ok {
                if !require_post {
                    return true;
                }
                let post_ok = i + tlen >= qb.len() || !qb[i + tlen].is_ascii_alphabetic();
                if post_ok { return true; }
            }
        }
        i += 1;
    }
    false
}

#[derive(Debug, Clone, PartialEq)]
pub enum SkillCategory {
    Core,     // always injected into the base system prompt
    Practice, // always a trigger candidate regardless of session scope
    Domain,   // only trigger-matchable when in session scope
}

#[derive(Debug, Clone)]
pub struct Skill {
    pub name:           String,
    pub description:    String,
    pub license:        Option<String>,
    pub priority:       u8,      // 0 = normal, 1-9 = higher priority (default 0)
    pub triggers:       Vec<String>,
    pub token_estimate: usize,
    pub content:        String,
    pub source:         SkillSource,
    pub category:       SkillCategory,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SkillSource {
    Bundled,           // shipped with the binary, lowest priority
    Global,            // ~/.zap/skills/
    Project,           // .zap/skills/  — high priority
    External(PathBuf), // user-configured extra path (.kiro/skills, etc.)
}

impl Skill {
    /// Core skills are injected into the system prompt every session.
    pub fn is_always_on(&self) -> bool {
        self.category == SkillCategory::Core
    }

    /// Returns true if any trigger keyword appears in the query (case-insensitive).
    ///
    /// Multi-word triggers must appear as consecutive whole words (not substring).
    /// Single-word triggers use the existing word-start boundary match so "review"
    /// fires on "reviewing" but not "preview".
    pub fn matches(&self, query: &str) -> bool {
        let lower = query.to_lowercase();
        self.triggers.iter().any(|t| {
            let t = t.to_lowercase();
            if t.contains(' ') {
                // Multi-word trigger: consecutive words with word boundaries
                let trig_words: Vec<&str> = t.split_whitespace().collect();
                if trig_words.is_empty() { return false; }
                let query_words: Vec<&str> = lower.split_whitespace().collect();
                query_words.windows(trig_words.len()).any(|w| w == trig_words.as_slice())
            } else {
                trigger_word_match(&lower, &t)
            }
        })
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
pub fn source_label(source: &SkillSource) -> String {
    match source {
        SkillSource::Bundled       => "built-in".to_string(),
        SkillSource::Global        => "global".to_string(),
        SkillSource::Project       => "project".to_string(),
        SkillSource::External(p)   => p.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| p.to_string_lossy().into_owned()),
    }
}

// ── Discovery ─────────────────────────────────────────────────────────────────

/// Expand `~` at the start of a path to the actual home directory.
fn expand_tilde(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/").or_else(|| p.strip_prefix("~\\")) {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(p)
}

/// Returns all skill directories to scan, in priority order (project overrides global).
/// `extra` comes from `config.skill_paths` — e.g. [".kiro/skills", "~/shared-skills"].
fn skill_dirs(extra: &[String]) -> Vec<(PathBuf, SkillSource)> {
    let mut dirs = Vec::new();

    // Global: ~/.zap/skills/
    if let Some(home) = dirs::home_dir() {
        let global = home.join(".zap").join("skills");
        if global.is_dir() {
            dirs.push((global, SkillSource::Global));
        }
    }

    // User-configured extra paths (inserted between global and project so project still wins).
    for raw in extra {
        let p = expand_tilde(raw);
        if p.is_dir() {
            dirs.push((p.clone(), SkillSource::External(p)));
        }
    }

    // Project-local: .zap/skills/ (highest priority, can override everything)
    let local = PathBuf::from(".zap").join("skills");
    if local.is_dir() {
        dirs.push((local, SkillSource::Project));
    }

    dirs
}

/// Load all skills: bundled defaults first, then global, then extra configured
/// paths, then project-local. Higher-priority same-name skills override lower ones.
pub fn load_all_skills(extra_dirs: &[String]) -> Vec<Skill> {
    // Start with bundled defaults.
    let mut skills: Vec<Skill> = bundled_skills();

    // Overlay user skills in priority order.
    for (dir, source) in skill_dirs(extra_dirs) {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                // Flat file: <name>.md
                if path.extension().and_then(|e| e.to_str()) == Some("md") {
                    match parse_skill_file(&path, source.clone()) {
                        Ok(skill) => {
                            if let Some(pos) = skills.iter().position(|s| s.name == skill.name) {
                                skills[pos] = skill;
                            } else {
                                skills.push(skill);
                            }
                        }
                        Err(e) => crate::zap_warn!("skill: could not parse {:?}: {}", path, e),
                    }
                // Kiro-style subdirectory: <name>/SKILL.md
                } else if path.is_dir() {
                    let skill_md = path.join("SKILL.md");
                    if skill_md.exists() {
                        let dir_name = path.file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown")
                            .to_string();
                        match parse_skill_file(&skill_md, source.clone()) {
                            Ok(mut skill) => {
                                skill.name = dir_name;
                                if let Some(pos) = skills.iter().position(|s| s.name == skill.name) {
                                    skills[pos] = skill;
                                } else {
                                    skills.push(skill);
                                }
                            }
                            Err(e) => crate::zap_warn!("skill: could not parse {:?}: {}", skill_md, e),
                        }
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

/// Sort matched skills by priority (desc), then source tier (project > external > global > bundled),
/// then token count (ascending — prefer smaller skills), and truncate to fit within `budget_tokens`.
/// Pinned skills are always kept (they count against the budget but are never dropped).
pub fn rank_and_truncate_skills<'a>(
    skills: Vec<&'a Skill>,
    budget_tokens: usize,
    pinned_names: &std::collections::HashSet<String>,
) -> Vec<&'a Skill> {
    if skills.is_empty() || budget_tokens == 0 {
        return skills;
    }

    // Separate pinned — they're non-negotiable.
    let (pinned, mut rest): (Vec<&Skill>, Vec<&Skill>) = skills
        .into_iter()
        .partition(|s| pinned_names.contains(&s.name));

    // Source tier ordinal: higher = more authoritative
    fn source_rank(source: &SkillSource) -> u8 {
        match source {
            SkillSource::Bundled       => 0,
            SkillSource::Global        => 1,
            SkillSource::External(_)   => 2,
            SkillSource::Project       => 3,
        }
    }

    // Sort rest: priority desc → source desc → tokens asc
    rest.sort_by(|a, b| {
        b.priority.cmp(&a.priority)
            .then_with(|| source_rank(&b.source).cmp(&source_rank(&a.source)))
            .then_with(|| a.tokens().cmp(&b.tokens()))
    });

    // Always keep pinned first
    let mut result: Vec<&Skill> = pinned;
    let mut used: usize = result.iter().map(|s| s.tokens()).sum();

    for skill in rest {
        let t = skill.tokens();
        if used + t <= budget_tokens || result.is_empty() {
            // Always keep at least one skill even if it exceeds budget
            result.push(skill);
            used += t;
        }
    }

    result
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
            let fm       = parse_frontmatter(&content_after_fence[..end_idx]);
            let body     = content_after_fence[end_idx + 4..].trim_start().to_string();
            let category = fm.category.unwrap_or({
                // Backwards compat: no triggers → Core; triggers present → Practice
                if fm.triggers.is_empty() { SkillCategory::Core } else { SkillCategory::Practice }
            });
            return Ok(Skill {
                name,
                description:    fm.description,
                license:        fm.license,
                priority:       fm.priority,
                triggers:       fm.triggers,
                token_estimate: fm.tokens,
                content:        body,
                source,
                category,
            });
        }
    }

    // No frontmatter — treat as Core (always-on), backwards compatible.
    Ok(Skill {
        name,
        description:    String::new(),
        license:        None,
        priority:       0,
        triggers:       Vec::new(),
        token_estimate: 0,
        content:        raw,
        source,
        category:       SkillCategory::Core,
    })
}

struct ParsedFrontmatter {
    description: String,
    license:     Option<String>,
    priority:    u8,           // 0 = normal, 1-9 = higher priority (default 0)
    triggers:    Vec<String>,
    tokens:      usize,
    category:    Option<SkillCategory>,
}

fn parse_frontmatter(fm: &str) -> ParsedFrontmatter {
    let mut description = String::new();
    let mut license     = None;
    let mut priority     = 0u8;
    let mut triggers    = Vec::new();
    let mut tokens      = 0usize;
    let mut category    = None;
    let mut in_triggers = false; // collecting YAML list items under triggers:/trigger:

    for line in fm.lines() {
        let line = line.trim();

        // YAML list item — only consumed when inside a triggers block
        if in_triggers && line.starts_with("- ") {
            let t = line[2..].trim().trim_matches('"').trim_matches('\'').to_string();
            if !t.is_empty() { triggers.push(t); }
            continue;
        }
        in_triggers = false;

        if line.starts_with("description:") {
            description = line.trim_start_matches("description:")
                .trim().trim_matches('"').trim_matches('\'').to_string();
        } else if line.starts_with("license:") {
            let lic = line.trim_start_matches("license:")
                .trim().trim_matches('"').trim_matches('\'').to_string();
            if !lic.is_empty() { license = Some(lic); }
        } else if line.starts_with("priority:") {
            let val = line.trim_start_matches("priority:").trim();
            let clean: String = val.chars().filter(|c| c.is_ascii_digit()).collect();
            priority = clean.parse().unwrap_or(0).min(9);
        } else if line.starts_with("triggers:") || line.starts_with("trigger:") {
            // "triggers:" is used by Claude Code; "trigger:" is zap-native — both supported
            let key_len = if line.starts_with("triggers:") { "triggers:".len() } else { "trigger:".len() };
            let val = line[key_len..].trim();
            if val.is_empty() {
                // YAML block list — items follow on subsequent lines
                in_triggers = true;
            } else {
                // Inline array: ["a", "b"]
                let inner = val.trim_matches(|c| c == '[' || c == ']');
                for part in inner.split(',') {
                    let t = part.trim().trim_matches('"').trim_matches('\'').to_string();
                    if !t.is_empty() { triggers.push(t); }
                }
            }
        } else if line.starts_with("tokens:") {
            let val = line.trim_start_matches("tokens:").trim();
            let clean: String = val.chars().filter(|c| c.is_ascii_digit()).collect();
            tokens = clean.parse().unwrap_or(0);
        } else if line.starts_with("category:") {
            let val = line.trim_start_matches("category:").trim().trim_matches('"').trim_matches('\'');
            category = match val {
                "core"     => Some(SkillCategory::Core),
                "practice" => Some(SkillCategory::Practice),
                "domain"   => Some(SkillCategory::Domain),
                _          => None,
            };
        }
    }

    ParsedFrontmatter { description, license, priority, triggers, tokens, category }
}

// ── Bundled default skills ────────────────────────────────────────────────────

/// Skills shipped with the binary. User skills of the same name override these.
fn bundled_skills() -> Vec<Skill> {
    const BUNDLED: &[(&str, &str)] = &[
        // Core — always injected
        ("karpathy-guidelines", include_str!("default_skills/karpathy-guidelines.md")),
        // Practice — always trigger-matchable, any session
        ("git",          include_str!("default_skills/git.md")),
        ("code-review",  include_str!("default_skills/code-review.md")),
        ("debugging",    include_str!("default_skills/debugging.md")),
        ("deploy",       include_str!("default_skills/deploy.md")),
        ("security",     include_str!("default_skills/security.md")),
        ("understand",   include_str!("default_skills/understand.md")),
        // Domain — session-scoped language/framework skills
        ("bash",         include_str!("default_skills/bash.md")),
        ("cpp",          include_str!("default_skills/cpp.md")),
        ("csharp",       include_str!("default_skills/csharp.md")),
        ("css",          include_str!("default_skills/css.md")),
        ("dart",         include_str!("default_skills/dart.md")),
        ("go",           include_str!("default_skills/go.md")),
        ("java",         include_str!("default_skills/java.md")),
        ("kotlin",       include_str!("default_skills/kotlin.md")),
        ("php",          include_str!("default_skills/php.md")),
        ("python",       include_str!("default_skills/python.md")),
        ("react",        include_str!("default_skills/react.md")),
        ("ruby",         include_str!("default_skills/ruby.md")),
        ("rust",         include_str!("default_skills/rust.md")),
        ("scala",        include_str!("default_skills/scala.md")),
        ("sql",          include_str!("default_skills/sql.md")),
        ("swift",        include_str!("default_skills/swift.md")),
        ("typescript",   include_str!("default_skills/typescript.md")),
        ("vue",          include_str!("default_skills/vue.md")),
    ];

    BUNDLED.iter().filter_map(|(name, raw)| {
        if let Some(after_fence) = raw.strip_prefix("---") {
            if let Some(end_idx) = after_fence.find("\n---") {
                let fm       = parse_frontmatter(&after_fence[..end_idx]);
                let body     = after_fence[end_idx + 4..].trim_start().to_string();
                let category = fm.category.unwrap_or(
                    if fm.triggers.is_empty() { SkillCategory::Core } else { SkillCategory::Practice }
                );
                return Some(Skill {
                    name:           name.to_string(),
                    description:    fm.description,
                    license:        fm.license,
                    priority:       fm.priority,
                    triggers:       fm.triggers,
                    token_estimate: fm.tokens,
                    content:        body,
                    source:         SkillSource::Bundled,
                    category,
                });
            }
        }
        None
    }).collect()
}

// ── Stack auto-detection ──────────────────────────────────────────────────────

/// Map manifest file → candidate domain skill names.
const STACK_MANIFESTS: &[(&str, &[&str])] = &[
    ("Cargo.toml",       &["rust"]),
    ("go.mod",           &["go"]),
    ("package.json",     &["typescript", "react", "vue"]),
    ("pyproject.toml",   &["python"]),
    ("setup.py",         &["python"]),
    ("pom.xml",          &["java"]),
    ("build.gradle",     &["java"]),
    ("build.gradle.kts", &["kotlin"]),
    ("Gemfile",          &["ruby"]),
    ("composer.json",    &["php"]),
    ("pubspec.yaml",     &["dart"]),
    ("CMakeLists.txt",   &["cpp"]),
    ("build.sbt",        &["scala"]),
];

/// Detect the project's tech stack and return skill names that match.
/// Used to auto-populate domain_scope at session startup.
pub fn detect_domain_scope(skills: &[Skill]) -> Vec<String> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let mut found: Vec<String> = Vec::new();

    for (manifest, candidates) in STACK_MANIFESTS {
        if cwd.join(manifest).exists() {
            for &candidate in *candidates {
                if skills.iter().any(|s| s.name == candidate && s.category == SkillCategory::Domain)
                    && !found.contains(&candidate.to_string())
                {
                    found.push(candidate.to_string());
                }
            }
        }
    }

    // C# — check for any .csproj file
    if std::fs::read_dir(&cwd).is_ok_and(|mut e| {
        e.any(|en| en.is_ok_and(|en| en.path().extension().is_some_and(|x| x == "csproj")))
    }) && skills.iter().any(|s| s.name == "csharp" && s.category == SkillCategory::Domain) {
        found.push("csharp".to_string());
    }

    found
}

/// Lightweight extension-based detection: scan files in cwd (non-recursive) and
/// return names of Domain skills whose typical extensions appear. Used for the
/// TUI domain picker to pre-check likely candidates without manifest files.
pub fn detect_from_extensions(skills: &[Skill]) -> Vec<String> {
    // Map skill name → file extensions that suggest it.
    const EXT_MAP: &[(&str, &[&str])] = &[
        ("rust",       &["rs"]),
        ("python",     &["py", "pyw", "ipynb"]),
        ("typescript", &["ts", "tsx"]),
        ("javascript", &["js", "jsx", "mjs", "cjs"]),
        ("go",         &["go"]),
        ("java",       &["java"]),
        ("kotlin",     &["kt", "kts"]),
        ("swift",      &["swift"]),
        ("ruby",       &["rb"]),
        ("cpp",        &["cpp", "cc", "cxx", "hpp", "hxx"]),
        ("c",          &["c", "h"]),
        ("csharp",     &["cs"]),
        ("php",        &["php"]),
        ("elixir",     &["ex", "exs"]),
        ("haskell",    &["hs", "lhs"]),
        ("scala",      &["scala"]),
        ("clojure",    &["clj", "cljs"]),
        ("dart",       &["dart"]),
        ("lua",        &["lua"]),
        ("zig",        &["zig"]),
    ];

    let cwd = std::env::current_dir().unwrap_or_default();
    let mut found_exts: std::collections::HashSet<String> = std::collections::HashSet::new();

    if let Ok(rd) = std::fs::read_dir(&cwd) {
        for entry in rd.flatten().take(200) {
            if let Some(ext) = entry.path().extension() {
                found_exts.insert(ext.to_string_lossy().to_lowercase());
            }
        }
    }

    let mut result = Vec::new();
    for (skill_name, exts) in EXT_MAP {
        if skills.iter().any(|s| s.name == *skill_name && s.category == SkillCategory::Domain)
            && exts.iter().any(|e| found_exts.contains(*e))
            && !result.contains(&skill_name.to_string())
        {
            result.push(skill_name.to_string());
        }
    }
    result
}

/// Backwards-compatible: returns skill refs for the startup banner.
pub fn detect_stack_skills(skills: &[Skill]) -> Vec<&Skill> {
    let names = detect_domain_scope(skills);
    skills.iter().filter(|s| names.contains(&s.name)).collect()
}

/// Match Practice skills + session-scoped Domain skills against a query.
/// `domain_scope` empty = no restriction (all Domain skills are candidates).
pub fn match_skills_scoped<'a>(
    query: &str,
    skills: &'a [Skill],
    domain_scope: &std::collections::HashSet<String>,
) -> Vec<&'a Skill> {
    let mut matched: Vec<&Skill> = Vec::new();
    for skill in skills {
        let eligible = match skill.category {
            SkillCategory::Core     => false, // already in system prompt
            SkillCategory::Practice => true,
            SkillCategory::Domain   => domain_scope.is_empty() || domain_scope.contains(&skill.name),
        };
        if eligible && skill.matches(query) {
            if let Some(existing) = matched.iter_mut().find(|s| s.name == skill.name) {
                if skill.source == SkillSource::Project { *existing = skill; }
            } else {
                matched.push(skill);
            }
        }
    }
    matched
}

/// All domain skills, sorted by name. Used to build the startup scope prompt.
pub fn all_domain_skills(skills: &[Skill]) -> Vec<&Skill> {
    let mut v: Vec<&Skill> = skills.iter()
        .filter(|s| s.category == SkillCategory::Domain)
        .collect();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

/// Returns sorted names of all Domain-category skills (used by TUI picker).
pub fn all_domain_skill_names(skills: &[Skill]) -> Vec<String> {
    all_domain_skills(skills).into_iter().map(|s| s.name.clone()).collect()
}

/// Show an interactive multi-select to let the user choose domain skills for this session.
/// Returns `None` if the user cancels (Esc) or selects nothing — callers treat that as
/// "no restriction" (all domain skills remain candidates).
pub fn prompt_domain_scope(skills: &[Skill]) -> Option<Vec<String>> {
    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() { return None; }

    let domain = all_domain_skills(skills);
    if domain.is_empty() { return None; }

    let options: Vec<String> = domain.iter().map(|s| s.name.clone()).collect();

    match inquire::MultiSelect::new(
        "Which languages/frameworks will you use this session?",
        options,
    )
    .with_help_message("↑↓ navigate  Space select  Enter confirm  Esc = no restriction")
    .with_render_config(crate::ui::inquire_render_config())
    .prompt()
    {
        Ok(selected) if !selected.is_empty() => Some(selected),
        _ => None,
    }
}

// ── First-run bootstrap ───────────────────────────────────────────────────────

/// Write bundled skills to `~/.zap/skills/` on first run (or when new built-ins
/// are added in a later release). Already-edited user files are never overwritten.
///
/// Returns the number of files written (0 = already up to date).
pub fn bootstrap_bundled_skills() -> usize {
    let dir = match dirs::home_dir() {
        Some(h) => h.join(".zap").join("skills"),
        None    => return 0,
    };
    if std::fs::create_dir_all(&dir).is_err() { return 0; }

    let mut written = 0usize;
    for skill in bundled_skills() {
        let dest = dir.join(format!("{}.md", skill.name));
        if dest.exists() { continue; }           // user may have edited it — never clobber
        if std::fs::write(&dest, skill_to_markdown(&skill)).is_ok() {
            written += 1;
        }
    }
    written
}

/// Serialize a `Skill` back to its `.md` frontmatter + body form.
pub fn skill_to_markdown(skill: &Skill) -> String {
    let triggers = skill.triggers
        .iter()
        .map(|t| format!("\"{}\"", t))
        .collect::<Vec<_>>()
        .join(", ");
    let category = match skill.category {
        SkillCategory::Core     => "core",
        SkillCategory::Practice => "practice",
        SkillCategory::Domain   => "domain",
    };
    let mut fm = format!(
        "---\nname: {}\ncategory: {}\n",
        skill.name, category
    );
    if !skill.description.is_empty() {
        fm.push_str(&format!("description: \"{}\"\n", skill.description));
    }
    if !triggers.is_empty() {
        fm.push_str(&format!("trigger: [{}]\n", triggers));
    }
    if skill.token_estimate > 0 {
        fm.push_str(&format!("tokens: {}\n", skill.token_estimate));
    }
    if let Some(ref lic) = skill.license {
        fm.push_str(&format!("license: \"{}\"\n", lic));
    }
    fm.push_str("---\n\n");
    fm.push_str(skill.content.trim());
    fm.push('\n');
    fm
}

/// Export a skill to `~/.zap/skills/<name>.md` so the user can view and edit it.
/// Returns the path written. Fails if the file already exists and `overwrite` is false.
pub fn export_skill(skill: &Skill, overwrite: bool) -> Result<PathBuf> {
    let dir = dirs::home_dir()
        .map(|h| h.join(".zap").join("skills"))
        .ok_or_else(|| anyhow::anyhow!("cannot locate home directory"))?;
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.md", skill.name));
    if path.exists() && !overwrite {
        anyhow::bail!(
            "'{}' already exists — pass --overwrite to replace it",
            path.display()
        );
    }
    std::fs::write(&path, skill_to_markdown(skill))?;
    Ok(path)
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

#[cfg(test)]
mod trigger_tests {
    use super::trigger_word_match;

    #[test]
    fn word_start_boundary() {
        // exact match
        assert!(trigger_word_match("code review", "review"));
        // at start of string
        assert!(trigger_word_match("review my code", "review"));
        // after punctuation
        assert!(trigger_word_match("please, review this", "review"));
        // inflected: "reviewing" still matches "review"
        assert!(trigger_word_match("i am reviewing the pr", "review"));
        // must NOT match mid-word ("preview")
        assert!(!trigger_word_match("preview the changes", "review"));
        // must NOT match mid-word ("irrelevant")
        assert!(!trigger_word_match("this is irrelevant", "review"));
        // short trigger "pr" must not match "process"
        assert!(!trigger_word_match("run the process", "pr"));
        // "pr" matches as standalone word
        assert!(trigger_word_match("the pr is ready", "pr"));
        assert!(trigger_word_match("pr review needed", "pr"));
    }

    // ── understand skill trigger coverage ────────────────────────────────────

    #[test]
    fn understand_what_is_this() {
        assert!(trigger_word_match("what is this project", "what is this"));
        assert!(trigger_word_match("what is this", "what is this"));
        // must NOT fire on a different "what" phrase
        assert!(!trigger_word_match("what should i do", "what is this"));
    }

    #[test]
    fn understand_explain() {
        assert!(trigger_word_match("can you explain the codebase", "explain the codebase"));
        assert!(trigger_word_match("explain this project to me", "explain this project"));
        assert!(trigger_word_match("explain the project briefly", "explain the project"));
        // "explaining" contains "explain" as a prefix — inflection fires
        assert!(trigger_word_match("start by explaining the structure", "explain"));
        // "explanation" diverges at char 5 ('n' vs 'i') — does NOT match "explain"
        assert!(!trigger_word_match("give an explanation", "explain"));
    }

    #[test]
    fn understand_summarize() {
        assert!(trigger_word_match("summarize the codebase", "summarize the codebase"));
        assert!(trigger_word_match("summarize this project", "summarize this"));
        assert!(trigger_word_match("can you summarize", "summarize"));
        // "summarizes" contains "summarize" as prefix — fires
        assert!(trigger_word_match("it summarizes each module", "summarize"));
        // "summarizing" diverges at char 8 ('i' vs 'e') — does NOT match "summarize"
        assert!(!trigger_word_match("summarizing the repo", "summarize"));
    }

    #[test]
    fn understand_overview() {
        assert!(trigger_word_match("give me a project overview", "project overview"));
        assert!(trigger_word_match("codebase overview please", "codebase overview"));
        assert!(trigger_word_match("give me an overview", "give me an overview"));
    }

    #[test]
    fn understand_architecture() {
        assert!(trigger_word_match("show me the architecture", "architecture"));
        assert!(trigger_word_match("architecture of this project", "architecture"));
        assert!(trigger_word_match("how is this structured", "how is this structured"));
        // must NOT fire on unrelated words containing "arch"
        assert!(!trigger_word_match("search the archive", "architecture"));
    }

    #[test]
    fn understand_docs() {
        assert!(trigger_word_match("generate docs for this", "generate docs"));
        assert!(trigger_word_match("generate documentation", "generate documentation"));
        // "generate" alone should not be a trigger — not in our list, so no test needed,
        // but verify multi-word triggers work as substrings
        assert!(trigger_word_match("please generate docs now", "generate docs"));
    }

    #[test]
    fn understand_onboard() {
        assert!(trigger_word_match("onboard me to this codebase", "onboard me"));
        assert!(trigger_word_match("orient me please", "orient me"));
        assert!(trigger_word_match("give me a tour of the code", "give me a tour"));
        assert!(trigger_word_match("walk me through the project", "walk me through"));
    }

    #[test]
    fn understand_how_is_structured() {
        assert!(trigger_word_match("how is this structured", "how is this structured"));
        assert!(trigger_word_match("how is it structured", "how is it structured"));
        // "how does zap work" is NOT in trigger list — does not fire
        assert!(!trigger_word_match("how does zap work exactly", "how is this structured"));
        assert!(!trigger_word_match("how does zap work exactly", "how is it structured"));
    }

    #[test]
    fn understand_no_false_positives() {
        // "understand" mid-word
        assert!(!trigger_word_match("misunderstanding the issue", "understand this"));
        // "overview" not triggered by "overview" appearing in unrelated context
        assert!(!trigger_word_match("reviewed the pull request overview doc already", "project overview"));
    }
}
