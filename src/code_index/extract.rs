use super::Symbol;

pub(super) struct RawSymbol {
    pub name:      String,
    pub kind:      String,
    pub line:      usize,
    pub signature: String,
    pub context:   String,
}

pub(super) fn extract_symbols(source: &str, lang: &str, path: &str) -> Vec<Symbol> {
    let raw = match lang {
        "rust"       => extract_rust(source),
        "python"     => extract_python(source),
        "javascript" => extract_js(source, false, false),
        "typescript" => extract_js(source, true, false),
        "tsx"        => extract_js(source, true, true),
        "go"         => extract_go(source),
        "java"       => extract_java(source),
        "csharp"     => super::extract_csharp::extract_csharp(source),
        _            => vec![],
    };

    raw.into_iter().map(|r| Symbol {
        path:      path.to_string(),
        name:      r.name,
        kind:      r.kind,
        line:      r.line,
        signature: r.signature,
        language:  lang.to_string(),
        context:   r.context,
    }).collect()
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

fn extract_rust(source: &str) -> Vec<RawSymbol> {
    let mut parser = match make_parser(tree_sitter_rust::language()) {
        Some(p) => p, None => return vec![],
    };
    let tree = match parser.parse(source.as_bytes(), None) {
        Some(t) => t, None => return vec![],
    };
    let mut out = Vec::new();
    extract_rust_node(tree.root_node(), source.as_bytes(), &mut out, "");
    out
}

fn extract_rust_node(node: tree_sitter::Node, src: &[u8], out: &mut Vec<RawSymbol>, context: &str) {
    let kind = node.kind();
    match kind {
        "function_item" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                let sig  = signature(node, src);
                out.push(RawSymbol { name: name.clone(), kind: "fn".into(), line: node.start_position().row + 1, signature: sig, context: context.to_string() });
                let new_ctx = format!("fn {}", name);
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) {
                        if c.kind() == "block" {
                            extract_rust_node(c, src, out, &new_ctx);
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
                    if let Some(c) = node.child(i) { extract_rust_node(c, src, out, &ctx); }
                }
                return;
            }
        }
        "impl_item" => {
            let impl_label = build_impl_label(node, src);
            for i in 0..node.child_count() {
                if let Some(c) = node.child(i) { extract_rust_node(c, src, out, &impl_label); }
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
        _ => {}
    }
    for i in 0..node.child_count() {
        if let Some(c) = node.child(i) { extract_rust_node(c, src, out, context); }
    }
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

fn extract_python(source: &str) -> Vec<RawSymbol> {
    let mut parser = match make_parser(tree_sitter_python::language()) {
        Some(p) => p, None => return vec![],
    };
    let tree = match parser.parse(source.as_bytes(), None) {
        Some(t) => t, None => return vec![],
    };
    let mut out = Vec::new();
    extract_python_node(tree.root_node(), source.as_bytes(), &mut out, "");
    out
}

fn extract_python_node(node: tree_sitter::Node, src: &[u8], out: &mut Vec<RawSymbol>, context: &str) {
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
                        if c.kind() == "block" { extract_python_node(c, src, out, &new_ctx); }
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
                    if let Some(c) = node.child(i) { extract_python_node(c, src, out, &new_ctx); }
                }
                return;
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() {
        if let Some(c) = node.child(i) { extract_python_node(c, src, out, context); }
    }
}

// ── JavaScript / TypeScript ───────────────────────────────────────────────────

fn extract_js(source: &str, typescript: bool, tsx: bool) -> Vec<RawSymbol> {
    let lang = if tsx {
        tree_sitter_typescript::language_tsx()
    } else if typescript {
        tree_sitter_typescript::language_typescript()
    } else {
        tree_sitter_javascript::language()
    };
    let mut parser = match make_parser(lang) { Some(p) => p, None => return vec![] };
    let tree = match parser.parse(source.as_bytes(), None) { Some(t) => t, None => return vec![] };
    let mut out = Vec::new();
    extract_js_node(tree.root_node(), source.as_bytes(), &mut out, "");
    out
}

fn extract_js_node(node: tree_sitter::Node, src: &[u8], out: &mut Vec<RawSymbol>, context: &str) {
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
                        if c.kind() == "statement_block" { extract_js_node(c, src, out, &new_ctx); }
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
                    if let Some(c) = node.child(i) { extract_js_node(c, src, out, &new_ctx); }
                }
                return;
            }
        }
        "method_definition" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name, kind: "method".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
                return;
            }
        }
        "interface_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name: name.clone(), kind: "interface".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
                let new_ctx = name.clone();
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) { extract_js_node(c, src, out, &new_ctx); }
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
            return;
        }
        _ => {}
    }
    for i in 0..node.child_count() {
        if let Some(c) = node.child(i) { extract_js_node(c, src, out, context); }
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

// ── Go ────────────────────────────────────────────────────────────────────────

fn extract_go(source: &str) -> Vec<RawSymbol> {
    let mut parser = match make_parser(tree_sitter_go::language()) {
        Some(p) => p, None => return vec![],
    };
    let tree = match parser.parse(source.as_bytes(), None) {
        Some(t) => t, None => return vec![],
    };
    let mut out = Vec::new();
    extract_go_node(tree.root_node(), source.as_bytes(), &mut out, "");
    out
}

fn extract_go_node(node: tree_sitter::Node, src: &[u8], out: &mut Vec<RawSymbol>, context: &str) {
    let kind = node.kind();
    match kind {
        "function_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name, kind: "func".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
                return;
            }
        }
        "method_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let recv = node.child_by_field_name("receiver")
                    .map(|r| node_text(r, src).trim_matches(|c| c == '(' || c == ')').trim().to_string())
                    .unwrap_or_default();
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name, kind: "method".into(), line: node.start_position().row + 1, signature: signature(node, src), context: recv });
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
        _ => {}
    }
    for i in 0..node.child_count() {
        if let Some(c) = node.child(i) { extract_go_node(c, src, out, context); }
    }
}

// ── Java ──────────────────────────────────────────────────────────────────────

fn extract_java(source: &str) -> Vec<RawSymbol> {
    let mut parser = match make_parser(tree_sitter_java::language()) {
        Some(p) => p, None => return vec![],
    };
    let tree = match parser.parse(source.as_bytes(), None) {
        Some(t) => t, None => return vec![],
    };
    let mut out = Vec::new();
    extract_java_node(tree.root_node(), source.as_bytes(), &mut out, "");
    out
}

fn extract_java_node(node: tree_sitter::Node, src: &[u8], out: &mut Vec<RawSymbol>, context: &str) {
    let kind = node.kind();
    match kind {
        "class_declaration" | "enum_declaration" | "record_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                let k = match kind { "enum_declaration" => "enum", "record_declaration" => "record", _ => "class" };
                out.push(RawSymbol { name: name.clone(), kind: k.into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
                let new_ctx = name.clone();
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) { extract_java_node(c, src, out, &new_ctx); }
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
                    if let Some(c) = node.child(i) { extract_java_node(c, src, out, &new_ctx); }
                }
                return;
            }
        }
        "method_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name, kind: "method".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
                return;
            }
        }
        "constructor_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name, kind: "constructor".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
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
        _ => {}
    }
    for i in 0..node.child_count() {
        if let Some(c) = node.child(i) { extract_java_node(c, src, out, context); }
    }
}
