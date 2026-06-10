use super::{CallSite, Import, Symbol, TypeEdge};

pub(super) struct RawSymbol {
    pub name:        String,
    pub kind:        String,
    pub line:        usize,
    pub signature:   String,
    pub context:     String,
    pub return_type: String,
    pub params:      String,  // JSON array of strings
}

pub(super) struct RawTypeEdge {
    pub child_name:  String,
    pub parent_name: String,
    pub edge_kind:   String,
    pub line:        usize,
}

pub(super) struct RawCallSite {
    pub line:          usize,
    pub col:           usize,
    pub name:          String,
    pub qualifier:     String,
    pub receiver_expr: String,
    pub caller_scope:  String,
}

pub(super) struct RawImport {
    pub line:          usize,
    pub module:        String,
    pub imported_name: String,
    pub alias:         String,
}

pub(super) struct ExtractResult {
    pub symbols:    Vec<Symbol>,
    pub call_sites: Vec<CallSite>,
    pub imports:    Vec<Import>,
    pub type_edges: Vec<TypeEdge>,
}

pub(super) fn extract_all(source: &str, lang: &str, path: &str) -> ExtractResult {
    let (raw_syms, raw_calls, raw_imports, raw_type_edges) = match lang {
        "rust"       => super::extract_rust::extract_rust(source),
        "python"     => super::extract_python::extract_python(source),
        "javascript" => super::extract_js::extract_js(source, false, false),
        "typescript" => super::extract_js::extract_js(source, true, false),
        "tsx"        => super::extract_js::extract_js(source, true, true),
        "go"         => super::extract_go::extract_go(source),
        "java"       => super::extract_java::extract_java(source),
        "csharp"     => super::extract_csharp::extract_csharp(source),
        _            => (vec![], vec![], vec![], vec![]),
    };

    let symbols = raw_syms.into_iter().map(|r| Symbol {
        path:        path.to_string(),
        name:        r.name,
        kind:        r.kind,
        line:        r.line,
        signature:   r.signature,
        language:    lang.to_string(),
        context:     r.context,
        return_type: r.return_type,
        params:      r.params,
    }).collect();

    let call_sites = raw_calls.into_iter().filter(|r| keep_call(&r.name)).map(|r| CallSite {
        path:          path.to_string(),
        line:          r.line,
        col:           r.col,
        name:          r.name,
        qualifier:     r.qualifier,
        receiver_expr: r.receiver_expr,
        caller_scope:  r.caller_scope,
        language:      lang.to_string(),
    }).collect();

    let imports = raw_imports.into_iter().map(|r| Import {
        path:          path.to_string(),
        line:          r.line,
        module:        r.module,
        imported_name: r.imported_name,
        alias:         r.alias,
        language:      lang.to_string(),
    }).collect();

    let type_edges = raw_type_edges.into_iter().map(|r| TypeEdge {
        id:          0,
        child_path:  path.to_string(),
        child_name:  r.child_name,
        parent_name: r.parent_name,
        edge_kind:   r.edge_kind,
        line:        r.line,
        language:    lang.to_string(),
    }).collect();

    ExtractResult { symbols, call_sites, imports, type_edges }
}

/// Serialize a list of parameter strings as a compact JSON array.
pub(super) fn params_to_json(params: &[&str]) -> String {
    if params.is_empty() { return "[]".into(); }
    let inner: Vec<String> = params.iter()
        .map(|s| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")))
        .collect();
    format!("[{}]", inner.join(","))
}

/// v1 noise filter: drop single-char identifiers. Per-language stdlib noise
/// (Rust macros) is filtered earlier inside the extractor.
fn keep_call(name: &str) -> bool {
    name.chars().count() >= 2
}

/// Stdlib Rust macros that are pure noise as call edges. Filtered at emission time
/// so `ref_count` and graph queries don't drown in `println!` references.
pub(super) const RUST_MACRO_NOISE: &[&str] = &[
    "println", "print", "eprintln", "eprint",
    "format", "write", "writeln",
    "vec", "dbg", "panic", "todo", "unimplemented", "unreachable",
    "assert", "assert_eq", "assert_ne",
    "debug_assert", "debug_assert_eq", "debug_assert_ne",
    "include", "include_str", "include_bytes",
    "env", "option_env", "cfg", "matches", "derive",
    "concat", "stringify", "line", "file", "module_path", "column",
];

pub(super) fn truncate_receiver(s: &str) -> String {
    let trimmed: String = s.chars().take(64).collect();
    if trimmed.chars().count() < s.chars().count() {
        format!("{}…", trimmed)
    } else {
        trimmed
    }
}

pub(super) fn make_parser(language: tree_sitter::Language) -> Option<tree_sitter::Parser> {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(language).ok()?;
    Some(parser)
}

pub(super) fn signature(node: tree_sitter::Node, source: &[u8]) -> String {
    let body_start = body_start(node);
    let end = body_start.unwrap_or(node.end_byte());
    let text = &source[node.start_byte()..end.min(source.len())];
    let s = std::str::from_utf8(text).unwrap_or("").trim();
    let collapsed: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > 200 {
        let truncated: String = collapsed.chars().take(200).collect();
        format!("{}…", truncated)
    } else {
        collapsed
    }
}

pub(super) fn body_start(node: tree_sitter::Node) -> Option<usize> {
    let body_kinds = &[
        "block", "statement_block", "suite", "declaration_list",
        "class_body", "enum_body", "field_declaration_list", "interface_body",
        "struct_body",
    ];
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if body_kinds.contains(&child.kind()) {
                return Some(child.start_byte());
            }
        }
    }
    None
}

pub(super) fn node_text<'a>(node: tree_sitter::Node, source: &'a [u8]) -> &'a str {
    std::str::from_utf8(&source[node.start_byte()..node.end_byte()]).unwrap_or("")
}
