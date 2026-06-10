use super::{CallSite, Import, Symbol};

pub(super) struct RawSymbol {
    pub name:      String,
    pub kind:      String,
    pub line:      usize,
    pub signature: String,
    pub context:   String,
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
}

pub(super) fn extract_all(source: &str, lang: &str, path: &str) -> ExtractResult {
    let (raw_syms, raw_calls, raw_imports) = match lang {
        "rust"       => extract_rust(source),
        "python"     => extract_python(source),
        "javascript" => extract_js(source, false, false),
        "typescript" => extract_js(source, true, false),
        "tsx"        => extract_js(source, true, true),
        "go"         => extract_go(source),
        "java"       => extract_java(source),
        "csharp"     => super::extract_csharp::extract_csharp(source),
        _            => (vec![], vec![], vec![]),
    };

    let symbols = raw_syms.into_iter().map(|r| Symbol {
        path:      path.to_string(),
        name:      r.name,
        kind:      r.kind,
        line:      r.line,
        signature: r.signature,
        language:  lang.to_string(),
        context:   r.context,
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

    ExtractResult { symbols, call_sites, imports }
}

/// v1 noise filter: drop single-char identifiers. Per-language stdlib noise
/// (Rust macros) is filtered earlier inside the extractor.
fn keep_call(name: &str) -> bool {
    name.chars().count() >= 2
}

/// Stdlib Rust macros that are pure noise as call edges. Filtered at emission time
/// so `ref_count` and graph queries don't drown in `println!` references.
const RUST_MACRO_NOISE: &[&str] = &[
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

// ── Rust ──────────────────────────────────────────────────────────────────────

fn extract_rust(source: &str) -> (Vec<RawSymbol>, Vec<RawCallSite>, Vec<RawImport>) {
    let mut parser = match make_parser(tree_sitter_rust::language()) {
        Some(p) => p, None => return (vec![], vec![], vec![]),
    };
    let tree = match parser.parse(source.as_bytes(), None) {
        Some(t) => t, None => return (vec![], vec![], vec![]),
    };
    let mut syms = Vec::new();
    let mut calls = Vec::new();
    let mut imports = Vec::new();
    extract_rust_node(tree.root_node(), source.as_bytes(), &mut syms, &mut calls, &mut imports, "");
    (syms, calls, imports)
}

fn extract_rust_node(
    node: tree_sitter::Node, src: &[u8],
    out: &mut Vec<RawSymbol>,
    calls: &mut Vec<RawCallSite>,
    imports: &mut Vec<RawImport>,
    context: &str,
) {
    let kind = node.kind();
    match kind {
        "function_item" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                let sig  = signature(node, src);
                out.push(RawSymbol { name: name.clone(), kind: "fn".into(), line: node.start_position().row + 1, signature: sig, context: context.to_string() });
                let new_ctx = if context.is_empty() {
                    format!("fn {}", name)
                } else {
                    format!("{} · {}", context, name)
                };
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) {
                        if c.kind() == "block" {
                            extract_rust_node(c, src, out, calls, imports, &new_ctx);
                        }
                    }
                }
                return;
            }
        }
        "struct_item" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name, kind: "struct".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
            }
        }
        "enum_item" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name, kind: "enum".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
            }
        }
        "trait_item" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                let ctx = format!("trait {}", name);
                out.push(RawSymbol { name: node_text(n, src).to_string(), kind: "trait".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) { extract_rust_node(c, src, out, calls, imports, &ctx); }
                }
                return;
            }
        }
        "impl_item" => {
            let impl_label = build_impl_label(node, src);
            for i in 0..node.child_count() {
                if let Some(c) = node.child(i) { extract_rust_node(c, src, out, calls, imports, &impl_label); }
            }
            return;
        }
        "const_item" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name, kind: "const".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
            }
        }
        "type_item" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name, kind: "type".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
            }
        }
        "macro_definition" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name, kind: "macro".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
            }
        }
        "call_expression" => {
            if let Some(cs) = parse_rust_call(node, src, context) {
                calls.push(cs);
            }
            // descend into args — nested calls live there
            for i in 0..node.child_count() {
                if let Some(c) = node.child(i) {
                    extract_rust_node(c, src, out, calls, imports, context);
                }
            }
            return;
        }
        "macro_invocation" => {
            if let Some(cs) = parse_rust_macro(node, src, context) {
                calls.push(cs);
            }
            for i in 0..node.child_count() {
                if let Some(c) = node.child(i) {
                    extract_rust_node(c, src, out, calls, imports, context);
                }
            }
            return;
        }
        "use_declaration" => {
            flatten_rust_use(node, src, imports);
            return;
        }
        _ => {}
    }
    for i in 0..node.child_count() {
        if let Some(c) = node.child(i) { extract_rust_node(c, src, out, calls, imports, context); }
    }
}

fn parse_rust_call(node: tree_sitter::Node, src: &[u8], context: &str) -> Option<RawCallSite> {
    let function = node.child_by_field_name("function")?;
    let (qualifier, name, receiver) = unwrap_rust_callable(function, src);
    if name.is_empty() { return None; }
    Some(RawCallSite {
        line:          function.start_position().row + 1,
        col:           function.start_position().column,
        name,
        qualifier,
        receiver_expr: receiver,
        caller_scope:  context.to_string(),
    })
}

fn unwrap_rust_callable(node: tree_sitter::Node, src: &[u8]) -> (String, String, String) {
    match node.kind() {
        "identifier" => ("".into(), node_text(node, src).to_string(), "".into()),
        "scoped_identifier" => {
            let path = node.child_by_field_name("path")
                .map(|n| node_text(n, src).to_string()).unwrap_or_default();
            let name = node.child_by_field_name("name")
                .map(|n| node_text(n, src).to_string()).unwrap_or_default();
            (path, name, "".into())
        }
        "field_expression" => {
            let recv = node.child_by_field_name("value")
                .map(|n| truncate_receiver(node_text(n, src))).unwrap_or_default();
            let name = node.child_by_field_name("field")
                .map(|n| node_text(n, src).to_string()).unwrap_or_default();
            ("".into(), name, recv)
        }
        "generic_function" => {
            if let Some(inner) = node.child_by_field_name("function") {
                return unwrap_rust_callable(inner, src);
            }
            ("".into(), "".into(), "".into())
        }
        _ => ("".into(), "".into(), "".into()),
    }
}

fn parse_rust_macro(node: tree_sitter::Node, src: &[u8], context: &str) -> Option<RawCallSite> {
    let mac = node.child_by_field_name("macro")?;
    let (qualifier, name, _) = unwrap_rust_callable(mac, src);
    if name.is_empty() { return None; }
    if RUST_MACRO_NOISE.contains(&name.as_str()) { return None; }
    Some(RawCallSite {
        line:          mac.start_position().row + 1,
        col:           mac.start_position().column,
        name,
        qualifier,
        receiver_expr: "".into(),
        caller_scope:  context.to_string(),
    })
}

fn flatten_rust_use(node: tree_sitter::Node, src: &[u8], imports: &mut Vec<RawImport>) {
    let line = node.start_position().row + 1;
    let arg = match node.child_by_field_name("argument") {
        Some(a) => a,
        None => return,
    };
    walk_rust_use_tree(arg, src, "", line, imports);
}

fn walk_rust_use_tree(node: tree_sitter::Node, src: &[u8], prefix: &str, line: usize, imports: &mut Vec<RawImport>) {
    match node.kind() {
        "identifier" | "self" | "super" | "crate" => {
            let name = node_text(node, src).to_string();
            imports.push(RawImport { line, module: prefix.to_string(), imported_name: name, alias: "".into() });
        }
        "scoped_identifier" => {
            let path = node.child_by_field_name("path")
                .map(|n| node_text(n, src).to_string()).unwrap_or_default();
            let name = node.child_by_field_name("name")
                .map(|n| node_text(n, src).to_string()).unwrap_or_default();
            let module = join_path(prefix, &path);
            imports.push(RawImport { line, module, imported_name: name, alias: "".into() });
        }
        "use_as_clause" => {
            let alias = node.child_by_field_name("alias")
                .map(|n| node_text(n, src).to_string()).unwrap_or_default();
            if let Some(path_node) = node.child_by_field_name("path") {
                let (module, name) = match path_node.kind() {
                    "scoped_identifier" => {
                        let path = path_node.child_by_field_name("path")
                            .map(|n| node_text(n, src).to_string()).unwrap_or_default();
                        let name = path_node.child_by_field_name("name")
                            .map(|n| node_text(n, src).to_string()).unwrap_or_default();
                        (join_path(prefix, &path), name)
                    }
                    _ => (prefix.to_string(), node_text(path_node, src).to_string()),
                };
                if !name.is_empty() {
                    imports.push(RawImport { line, module, imported_name: name, alias });
                }
            }
        }
        "scoped_use_list" => {
            let path = node.child_by_field_name("path")
                .map(|n| node_text(n, src).to_string()).unwrap_or_default();
            let new_prefix = join_path(prefix, &path);
            if let Some(list) = node.child_by_field_name("list") {
                walk_rust_use_tree(list, src, &new_prefix, line, imports);
            }
        }
        "use_list" => {
            for i in 0..node.child_count() {
                if let Some(c) = node.child(i) {
                    if c.is_named() {
                        walk_rust_use_tree(c, src, prefix, line, imports);
                    }
                }
            }
        }
        "use_wildcard" => {
            let inner = (0..node.child_count())
                .filter_map(|i| node.child(i))
                .find(|c| c.is_named() && c.kind() != "use_wildcard");
            let module = inner
                .map(|n| join_path(prefix, node_text(n, src)))
                .unwrap_or_else(|| prefix.to_string());
            imports.push(RawImport { line, module, imported_name: "*".into(), alias: "".into() });
        }
        _ => {
            for i in 0..node.child_count() {
                if let Some(c) = node.child(i) {
                    if c.is_named() {
                        walk_rust_use_tree(c, src, prefix, line, imports);
                    }
                }
            }
        }
    }
}

fn join_path(prefix: &str, segment: &str) -> String {
    if prefix.is_empty()      { segment.to_string() }
    else if segment.is_empty() { prefix.to_string() }
    else                       { format!("{}::{}", prefix, segment) }
}

fn build_impl_label(node: tree_sitter::Node, src: &[u8]) -> String {
    let mut parts = vec!["impl".to_string()];
    for i in 0..node.child_count() {
        if let Some(c) = node.child(i) {
            match c.kind() {
                "for" => { parts.push("for".into()); }
                "type_identifier" | "generic_type" | "scoped_type_identifier" => {
                    parts.push(node_text(c, src).to_string());
                }
                "declaration_list" | "where_clause" => break,
                _ => {}
            }
        }
    }
    parts.join(" ")
}

// ── Python ────────────────────────────────────────────────────────────────────

fn extract_python(source: &str) -> (Vec<RawSymbol>, Vec<RawCallSite>, Vec<RawImport>) {
    let mut parser = match make_parser(tree_sitter_python::language()) {
        Some(p) => p, None => return (vec![], vec![], vec![]),
    };
    let tree = match parser.parse(source.as_bytes(), None) {
        Some(t) => t, None => return (vec![], vec![], vec![]),
    };
    let mut syms = Vec::new();
    let mut calls = Vec::new();
    let mut imports = Vec::new();
    extract_python_node(tree.root_node(), source.as_bytes(), &mut syms, &mut calls, &mut imports, "");
    (syms, calls, imports)
}

fn extract_python_node(
    node: tree_sitter::Node, src: &[u8],
    out: &mut Vec<RawSymbol>,
    calls: &mut Vec<RawCallSite>,
    imports: &mut Vec<RawImport>,
    context: &str,
) {
    let kind = node.kind();
    match kind {
        "function_definition" | "async_function_definition" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                let k = if kind == "async_function_definition" { "async fn" } else { "def" };
                out.push(RawSymbol { name: name.clone(), kind: k.into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
                let new_ctx = if context.is_empty() { name.clone() } else { format!("{}.{}", context, name) };
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) {
                        if c.kind() == "block" { extract_python_node(c, src, out, calls, imports, &new_ctx); }
                    }
                }
                return;
            }
        }
        "class_definition" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name: name.clone(), kind: "class".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
                let new_ctx = if context.is_empty() { name.clone() } else { format!("{}.{}", context, name) };
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) { extract_python_node(c, src, out, calls, imports, &new_ctx); }
                }
                return;
            }
        }
        "call" => {
            if let Some(cs) = parse_python_call(node, src, context) {
                calls.push(cs);
            }
            for i in 0..node.child_count() {
                if let Some(c) = node.child(i) {
                    extract_python_node(c, src, out, calls, imports, context);
                }
            }
            return;
        }
        "import_statement" => {
            flatten_python_import(node, src, imports, false);
            return;
        }
        "import_from_statement" => {
            flatten_python_import(node, src, imports, true);
            return;
        }
        _ => {}
    }
    for i in 0..node.child_count() {
        if let Some(c) = node.child(i) { extract_python_node(c, src, out, calls, imports, context); }
    }
}

fn parse_python_call(node: tree_sitter::Node, src: &[u8], context: &str) -> Option<RawCallSite> {
    let function = node.child_by_field_name("function")?;
    let (qualifier, name, receiver) = unwrap_python_callable(function, src);
    if name.is_empty() { return None; }
    Some(RawCallSite {
        line:          function.start_position().row + 1,
        col:           function.start_position().column,
        name,
        qualifier,
        receiver_expr: receiver,
        caller_scope:  context.to_string(),
    })
}

fn unwrap_python_callable(node: tree_sitter::Node, src: &[u8]) -> (String, String, String) {
    match node.kind() {
        "identifier" => ("".into(), node_text(node, src).to_string(), "".into()),
        "attribute" => {
            // obj.method(...) — object field + attribute field
            let obj = node.child_by_field_name("object");
            let attr = node.child_by_field_name("attribute")
                .map(|n| node_text(n, src).to_string()).unwrap_or_default();
            // If object is itself an attribute (a.b.c), treat the full object text as qualifier
            // for module-style calls (os.path.join), receiver for value-style.
            // v1 heuristic: if object text contains only identifiers + dots, it's likely a module path.
            let obj_text = obj.map(|n| node_text(n, src).to_string()).unwrap_or_default();
            let looks_like_path = !obj_text.is_empty()
                && obj_text.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
            if looks_like_path {
                (obj_text, attr, "".into())
            } else {
                ("".into(), attr, truncate_receiver(&obj_text))
            }
        }
        _ => ("".into(), "".into(), "".into()),
    }
}

fn flatten_python_import(node: tree_sitter::Node, src: &[u8], imports: &mut Vec<RawImport>, is_from: bool) {
    let line = node.start_position().row + 1;
    if is_from {
        let module = node.child_by_field_name("module_name")
            .map(|n| node_text(n, src).to_string()).unwrap_or_default();
        // from X import a, b as c, *
        for i in 0..node.child_count() {
            if let Some(c) = node.child(i) {
                match c.kind() {
                    "dotted_name" | "identifier" => {
                        let text = node_text(c, src).to_string();
                        // Skip the module_name itself (same node referenced by field)
                        if Some(c.id()) == node.child_by_field_name("module_name").map(|n| n.id()) {
                            continue;
                        }
                        imports.push(RawImport {
                            line,
                            module: module.clone(),
                            imported_name: text,
                            alias: "".into(),
                        });
                    }
                    "aliased_import" => {
                        let name = c.child_by_field_name("name")
                            .map(|n| node_text(n, src).to_string()).unwrap_or_default();
                        let alias = c.child_by_field_name("alias")
                            .map(|n| node_text(n, src).to_string()).unwrap_or_default();
                        if !name.is_empty() {
                            imports.push(RawImport { line, module: module.clone(), imported_name: name, alias });
                        }
                    }
                    "wildcard_import" => {
                        imports.push(RawImport {
                            line,
                            module: module.clone(),
                            imported_name: "*".into(),
                            alias: "".into(),
                        });
                    }
                    _ => {}
                }
            }
        }
    } else {
        // import X, Y as Z
        for i in 0..node.child_count() {
            if let Some(c) = node.child(i) {
                match c.kind() {
                    "dotted_name" | "identifier" => {
                        let module = node_text(c, src).to_string();
                        if !module.is_empty() {
                            imports.push(RawImport {
                                line,
                                module,
                                imported_name: "".into(),
                                alias: "".into(),
                            });
                        }
                    }
                    "aliased_import" => {
                        let module = c.child_by_field_name("name")
                            .map(|n| node_text(n, src).to_string()).unwrap_or_default();
                        let alias = c.child_by_field_name("alias")
                            .map(|n| node_text(n, src).to_string()).unwrap_or_default();
                        if !module.is_empty() {
                            imports.push(RawImport {
                                line,
                                module,
                                imported_name: "".into(),
                                alias,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

// ── JavaScript / TypeScript ───────────────────────────────────────────────────

fn extract_js(source: &str, typescript: bool, tsx: bool) -> (Vec<RawSymbol>, Vec<RawCallSite>, Vec<RawImport>) {
    let lang = if tsx {
        tree_sitter_typescript::language_tsx()
    } else if typescript {
        tree_sitter_typescript::language_typescript()
    } else {
        tree_sitter_javascript::language()
    };
    let mut parser = match make_parser(lang) { Some(p) => p, None => return (vec![], vec![], vec![]) };
    let tree = match parser.parse(source.as_bytes(), None) { Some(t) => t, None => return (vec![], vec![], vec![]) };
    let mut syms = Vec::new();
    let mut calls = Vec::new();
    let mut imports = Vec::new();
    extract_js_node(tree.root_node(), source.as_bytes(), &mut syms, &mut calls, &mut imports, "");
    (syms, calls, imports)
}

fn extract_js_node(
    node: tree_sitter::Node, src: &[u8],
    out: &mut Vec<RawSymbol>,
    calls: &mut Vec<RawCallSite>,
    imports: &mut Vec<RawImport>,
    context: &str,
) {
    let kind = node.kind();
    match kind {
        "function_declaration" | "generator_function_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                let k = if kind == "generator_function_declaration" { "function*" } else { "function" };
                out.push(RawSymbol { name: name.clone(), kind: k.into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
                let new_ctx = name.clone();
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) {
                        if c.kind() == "statement_block" { extract_js_node(c, src, out, calls, imports, &new_ctx); }
                    }
                }
                return;
            }
        }
        "class_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name: name.clone(), kind: "class".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
                let new_ctx = name.clone();
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) { extract_js_node(c, src, out, calls, imports, &new_ctx); }
                }
                return;
            }
        }
        "method_definition" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name: name.clone(), kind: "method".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
                let new_ctx = if context.is_empty() { name } else { format!("{}.{}", context, name) };
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) {
                        if c.kind() == "statement_block" { extract_js_node(c, src, out, calls, imports, &new_ctx); }
                    }
                }
                return;
            }
        }
        "interface_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name: name.clone(), kind: "interface".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
                let new_ctx = name.clone();
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) { extract_js_node(c, src, out, calls, imports, &new_ctx); }
                }
                return;
            }
        }
        "type_alias_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name, kind: "type".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
            }
        }
        "enum_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name, kind: "enum".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
            }
        }
        "lexical_declaration" | "variable_declaration" => {
            extract_js_var_decls(node, src, out, context);
            // also descend — initializers can contain calls
            for i in 0..node.child_count() {
                if let Some(c) = node.child(i) {
                    extract_js_node(c, src, out, calls, imports, context);
                }
            }
            return;
        }
        "call_expression" | "new_expression" => {
            if let Some(cs) = parse_js_call(node, src, context) {
                calls.push(cs);
            }
            for i in 0..node.child_count() {
                if let Some(c) = node.child(i) {
                    extract_js_node(c, src, out, calls, imports, context);
                }
            }
            return;
        }
        "import_statement" => {
            flatten_js_import(node, src, imports);
            return;
        }
        _ => {}
    }
    for i in 0..node.child_count() {
        if let Some(c) = node.child(i) { extract_js_node(c, src, out, calls, imports, context); }
    }
}

fn extract_js_var_decls(node: tree_sitter::Node, src: &[u8], out: &mut Vec<RawSymbol>, context: &str) {
    for i in 0..node.child_count() {
        if let Some(decl) = node.child(i) {
            if decl.kind() == "variable_declarator" {
                if let (Some(name_node), Some(val_node)) = (
                    decl.child_by_field_name("name"),
                    decl.child_by_field_name("value"),
                ) {
                    let val_kind = val_node.kind();
                    if matches!(val_kind, "arrow_function" | "function" | "generator_function") {
                        let name = node_text(name_node, src).to_string();
                        let k = if val_kind == "arrow_function" { "arrow fn" } else { "function" };
                        out.push(RawSymbol {
                            name, kind: k.into(),
                            line: decl.start_position().row + 1,
                            signature: signature(decl, src),
                            context: context.to_string(),
                        });
                    }
                }
            }
        }
    }
}

fn parse_js_call(node: tree_sitter::Node, src: &[u8], context: &str) -> Option<RawCallSite> {
    // For new_expression the callable is in field "constructor"; for call_expression it's "function".
    let function = node.child_by_field_name("function")
        .or_else(|| node.child_by_field_name("constructor"))?;
    let (qualifier, name, receiver) = unwrap_js_callable(function, src);
    if name.is_empty() { return None; }
    Some(RawCallSite {
        line:          function.start_position().row + 1,
        col:           function.start_position().column,
        name,
        qualifier,
        receiver_expr: receiver,
        caller_scope:  context.to_string(),
    })
}

fn unwrap_js_callable(node: tree_sitter::Node, src: &[u8]) -> (String, String, String) {
    match node.kind() {
        "identifier" | "type_identifier" => ("".into(), node_text(node, src).to_string(), "".into()),
        "member_expression" => {
            let obj = node.child_by_field_name("object");
            let prop = node.child_by_field_name("property")
                .map(|n| node_text(n, src).to_string()).unwrap_or_default();
            let obj_text = obj.map(|n| node_text(n, src).to_string()).unwrap_or_default();
            // Module-style heuristic: if object looks like a bare identifier or dotted path, treat as qualifier.
            let looks_like_path = !obj_text.is_empty()
                && obj_text.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '$');
            if looks_like_path {
                (obj_text, prop, "".into())
            } else {
                ("".into(), prop, truncate_receiver(&obj_text))
            }
        }
        _ => ("".into(), "".into(), "".into()),
    }
}

fn flatten_js_import(node: tree_sitter::Node, src: &[u8], imports: &mut Vec<RawImport>) {
    let line = node.start_position().row + 1;
    let source_node = (0..node.child_count())
        .filter_map(|i| node.child(i))
        .find(|c| c.kind() == "string");
    let module = source_node
        .map(|n| node_text(n, src).trim_matches(|c| c == '"' || c == '\'' || c == '`').to_string())
        .unwrap_or_default();
    if module.is_empty() { return; }

    // import "foo" — side-effect only
    let import_clause = (0..node.child_count())
        .filter_map(|i| node.child(i))
        .find(|c| c.kind() == "import_clause");
    let Some(clause) = import_clause else {
        imports.push(RawImport { line, module, imported_name: "".into(), alias: "".into() });
        return;
    };

    for i in 0..clause.child_count() {
        let Some(child) = clause.child(i) else { continue };
        match child.kind() {
            "identifier" => {
                // default import: import Foo from "x"
                imports.push(RawImport {
                    line,
                    module: module.clone(),
                    imported_name: "default".into(),
                    alias: node_text(child, src).to_string(),
                });
            }
            "namespace_import" => {
                // import * as ns from "x"
                let alias = (0..child.child_count())
                    .filter_map(|j| child.child(j))
                    .find(|c| c.kind() == "identifier")
                    .map(|n| node_text(n, src).to_string()).unwrap_or_default();
                imports.push(RawImport { line, module: module.clone(), imported_name: "*".into(), alias });
            }
            "named_imports" => {
                for j in 0..child.child_count() {
                    if let Some(spec) = child.child(j) {
                        if spec.kind() == "import_specifier" {
                            let name = spec.child_by_field_name("name")
                                .map(|n| node_text(n, src).to_string()).unwrap_or_default();
                            let alias = spec.child_by_field_name("alias")
                                .map(|n| node_text(n, src).to_string()).unwrap_or_default();
                            if !name.is_empty() {
                                imports.push(RawImport { line, module: module.clone(), imported_name: name, alias });
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

// ── Go ────────────────────────────────────────────────────────────────────────

fn extract_go(source: &str) -> (Vec<RawSymbol>, Vec<RawCallSite>, Vec<RawImport>) {
    let mut parser = match make_parser(tree_sitter_go::language()) {
        Some(p) => p, None => return (vec![], vec![], vec![]),
    };
    let tree = match parser.parse(source.as_bytes(), None) {
        Some(t) => t, None => return (vec![], vec![], vec![]),
    };
    let mut syms = Vec::new();
    let mut calls = Vec::new();
    let mut imports = Vec::new();
    extract_go_node(tree.root_node(), source.as_bytes(), &mut syms, &mut calls, &mut imports, "");
    (syms, calls, imports)
}

fn extract_go_node(
    node: tree_sitter::Node, src: &[u8],
    out: &mut Vec<RawSymbol>,
    calls: &mut Vec<RawCallSite>,
    imports: &mut Vec<RawImport>,
    context: &str,
) {
    let kind = node.kind();
    match kind {
        "function_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name: name.clone(), kind: "func".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
                let new_ctx = format!("func {}", name);
                if let Some(body) = node.child_by_field_name("body") {
                    extract_go_node(body, src, out, calls, imports, &new_ctx);
                }
                return;
            }
        }
        "method_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let recv = node.child_by_field_name("receiver")
                    .map(|r| node_text(r, src).trim_matches(|c| c == '(' || c == ')').trim().to_string())
                    .unwrap_or_default();
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name: name.clone(), kind: "method".into(), line: node.start_position().row + 1, signature: signature(node, src), context: recv.clone() });
                let new_ctx = if recv.is_empty() { format!("method {}", name) } else { format!("{} · {}", recv, name) };
                if let Some(body) = node.child_by_field_name("body") {
                    extract_go_node(body, src, out, calls, imports, &new_ctx);
                }
                return;
            }
        }
        "type_declaration" => {
            for i in 0..node.child_count() {
                if let Some(spec) = node.child(i) {
                    if spec.kind() == "type_spec" {
                        if let Some(n) = spec.child_by_field_name("name") {
                            let name = node_text(n, src).to_string();
                            let type_kind = spec.child_by_field_name("type").map(|t| t.kind()).unwrap_or("type");
                            let k = match type_kind {
                                "struct_type"    => "struct",
                                "interface_type" => "interface",
                                _                => "type",
                            };
                            out.push(RawSymbol { name, kind: k.into(), line: spec.start_position().row + 1, signature: signature(spec, src), context: context.to_string() });
                        }
                    }
                }
            }
            return;
        }
        "const_declaration" | "var_declaration" => {
            let k = if kind == "const_declaration" { "const" } else { "var" };
            for i in 0..node.child_count() {
                if let Some(spec) = node.child(i) {
                    if matches!(spec.kind(), "const_spec" | "var_spec") {
                        if let Some(n) = spec.child_by_field_name("name") {
                            let name = node_text(n, src).to_string();
                            out.push(RawSymbol { name, kind: k.into(), line: spec.start_position().row + 1, signature: signature(spec, src), context: context.to_string() });
                        }
                    }
                }
            }
            return;
        }
        "call_expression" => {
            if let Some(cs) = parse_go_call(node, src, context) {
                calls.push(cs);
            }
            for i in 0..node.child_count() {
                if let Some(c) = node.child(i) {
                    extract_go_node(c, src, out, calls, imports, context);
                }
            }
            return;
        }
        "import_declaration" => {
            flatten_go_imports(node, src, imports);
            return;
        }
        _ => {}
    }
    for i in 0..node.child_count() {
        if let Some(c) = node.child(i) { extract_go_node(c, src, out, calls, imports, context); }
    }
}

fn parse_go_call(node: tree_sitter::Node, src: &[u8], context: &str) -> Option<RawCallSite> {
    let function = node.child_by_field_name("function")?;
    let (qualifier, name, receiver) = unwrap_go_callable(function, src);
    if name.is_empty() { return None; }
    Some(RawCallSite {
        line:          function.start_position().row + 1,
        col:           function.start_position().column,
        name,
        qualifier,
        receiver_expr: receiver,
        caller_scope:  context.to_string(),
    })
}

fn unwrap_go_callable(node: tree_sitter::Node, src: &[u8]) -> (String, String, String) {
    match node.kind() {
        "identifier" => ("".into(), node_text(node, src).to_string(), "".into()),
        "selector_expression" => {
            // e.g. pkg.Func(...) or x.Method(...)
            let operand = node.child_by_field_name("operand");
            let field = node.child_by_field_name("field")
                .map(|n| node_text(n, src).to_string()).unwrap_or_default();
            let op_text = operand.map(|n| node_text(n, src).to_string()).unwrap_or_default();
            // Package-style heuristic (mirrors Python/JS attribute heuristic).
            let looks_like_path = !op_text.is_empty()
                && op_text.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
            if looks_like_path {
                (op_text, field, "".into())
            } else {
                ("".into(), field, truncate_receiver(&op_text))
            }
        }
        _ => ("".into(), "".into(), "".into()),
    }
}

fn flatten_go_imports(node: tree_sitter::Node, src: &[u8], imports: &mut Vec<RawImport>) {
    // Forms:
    //   import "fmt"
    //   import f "fmt"
    //   import ( "fmt"; f "fmt"; . "fmt" )
    fn capture_spec(spec: tree_sitter::Node, src: &[u8], imports: &mut Vec<RawImport>) {
        let line = spec.start_position().row + 1;
        let path_node = spec.child_by_field_name("path");
        let name_node = spec.child_by_field_name("name");
        let Some(p) = path_node else { return };
        let module = node_text(p, src).trim_matches(|c| c == '"' || c == '`').to_string();
        if module.is_empty() { return; }
        let alias = name_node.map(|n| node_text(n, src).to_string()).unwrap_or_default();
        imports.push(RawImport { line, module, imported_name: "".into(), alias });
    }
    for i in 0..node.child_count() {
        if let Some(c) = node.child(i) {
            match c.kind() {
                "import_spec" => capture_spec(c, src, imports),
                "import_spec_list" => {
                    for j in 0..c.child_count() {
                        if let Some(inner) = c.child(j) {
                            if inner.kind() == "import_spec" { capture_spec(inner, src, imports); }
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

// ── Java ──────────────────────────────────────────────────────────────────────

fn extract_java(source: &str) -> (Vec<RawSymbol>, Vec<RawCallSite>, Vec<RawImport>) {
    let mut parser = match make_parser(tree_sitter_java::language()) {
        Some(p) => p, None => return (vec![], vec![], vec![]),
    };
    let tree = match parser.parse(source.as_bytes(), None) {
        Some(t) => t, None => return (vec![], vec![], vec![]),
    };
    let mut syms = Vec::new();
    let mut calls = Vec::new();
    let mut imports = Vec::new();
    extract_java_node(tree.root_node(), source.as_bytes(), &mut syms, &mut calls, &mut imports, "");
    (syms, calls, imports)
}

fn extract_java_node(
    node: tree_sitter::Node, src: &[u8],
    out: &mut Vec<RawSymbol>,
    calls: &mut Vec<RawCallSite>,
    imports: &mut Vec<RawImport>,
    context: &str,
) {
    let kind = node.kind();
    match kind {
        "class_declaration" | "enum_declaration" | "record_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                let k = match kind { "enum_declaration" => "enum", "record_declaration" => "record", _ => "class" };
                out.push(RawSymbol { name: name.clone(), kind: k.into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
                let new_ctx = name.clone();
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) { extract_java_node(c, src, out, calls, imports, &new_ctx); }
                }
                return;
            }
        }
        "interface_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name: name.clone(), kind: "interface".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
                let new_ctx = name.clone();
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) { extract_java_node(c, src, out, calls, imports, &new_ctx); }
                }
                return;
            }
        }
        "method_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name: name.clone(), kind: "method".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
                let new_ctx = if context.is_empty() { name } else { format!("{} · {}", context, name) };
                if let Some(body) = node.child_by_field_name("body") {
                    extract_java_node(body, src, out, calls, imports, &new_ctx);
                }
                return;
            }
        }
        "constructor_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name: name.clone(), kind: "constructor".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
                let new_ctx = if context.is_empty() { name } else { format!("{} · ctor", context) };
                if let Some(body) = node.child_by_field_name("body") {
                    extract_java_node(body, src, out, calls, imports, &new_ctx);
                }
                return;
            }
        }
        "field_declaration" => {
            let text = node_text(node, src);
            if text.contains("static") && text.contains("final") {
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) {
                        if c.kind() == "variable_declarator" {
                            if let Some(n) = c.child_by_field_name("name") {
                                let name = node_text(n, src).to_string();
                                out.push(RawSymbol { name, kind: "const".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
                            }
                        }
                    }
                }
            }
        }
        "method_invocation" => {
            if let Some(cs) = parse_java_invocation(node, src, context) {
                calls.push(cs);
            }
            for i in 0..node.child_count() {
                if let Some(c) = node.child(i) {
                    extract_java_node(c, src, out, calls, imports, context);
                }
            }
            return;
        }
        "object_creation_expression" => {
            if let Some(cs) = parse_java_object_creation(node, src, context) {
                calls.push(cs);
            }
            for i in 0..node.child_count() {
                if let Some(c) = node.child(i) {
                    extract_java_node(c, src, out, calls, imports, context);
                }
            }
            return;
        }
        "import_declaration" => {
            flatten_java_import(node, src, imports);
            return;
        }
        _ => {}
    }
    for i in 0..node.child_count() {
        if let Some(c) = node.child(i) { extract_java_node(c, src, out, calls, imports, context); }
    }
}

fn parse_java_invocation(node: tree_sitter::Node, src: &[u8], context: &str) -> Option<RawCallSite> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, src).to_string();
    if name.is_empty() { return None; }
    let object = node.child_by_field_name("object");
    let (qualifier, receiver_expr) = match object {
        Some(o) => {
            let txt = node_text(o, src).to_string();
            let looks_like_path = !txt.is_empty()
                && txt.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
            if looks_like_path { (txt, "".into()) } else { ("".into(), truncate_receiver(&txt)) }
        }
        None => ("".into(), "".into()),
    };
    Some(RawCallSite {
        line:          name_node.start_position().row + 1,
        col:           name_node.start_position().column,
        name,
        qualifier,
        receiver_expr,
        caller_scope:  context.to_string(),
    })
}

fn parse_java_object_creation(node: tree_sitter::Node, src: &[u8], context: &str) -> Option<RawCallSite> {
    // `new Foo(...)` — record as call to `Foo`. Useful for "who constructs X" queries.
    let type_node = node.child_by_field_name("type")?;
    let raw = node_text(type_node, src).to_string();
    let name = raw.split('<').next().unwrap_or(&raw).trim().to_string();
    if name.is_empty() { return None; }
    Some(RawCallSite {
        line:          type_node.start_position().row + 1,
        col:           type_node.start_position().column,
        name,
        qualifier:     "new".into(),
        receiver_expr: "".into(),
        caller_scope:  context.to_string(),
    })
}

fn flatten_java_import(node: tree_sitter::Node, src: &[u8], imports: &mut Vec<RawImport>) {
    let line = node.start_position().row + 1;
    // Walk for the scoped_identifier (or identifier for top-level packages) and detect '*' for wildcard.
    let mut path_text = String::new();
    let mut is_wildcard = false;
    for i in 0..node.child_count() {
        if let Some(c) = node.child(i) {
            match c.kind() {
                "scoped_identifier" | "identifier" => {
                    if path_text.is_empty() {
                        path_text = node_text(c, src).to_string();
                    }
                }
                "asterisk" => { is_wildcard = true; }
                _ => {}
            }
        }
    }
    if path_text.is_empty() { return; }
    if is_wildcard {
        imports.push(RawImport { line, module: path_text, imported_name: "*".into(), alias: "".into() });
    } else if let Some((module, name)) = path_text.rsplit_once('.') {
        imports.push(RawImport { line, module: module.to_string(), imported_name: name.to_string(), alias: "".into() });
    } else {
        imports.push(RawImport { line, module: "".into(), imported_name: path_text, alias: "".into() });
    }
}
