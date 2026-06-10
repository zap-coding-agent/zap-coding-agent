use super::extract::{make_parser, node_text, params_to_json, signature, truncate_receiver, RawCallSite, RawImport, RawSymbol, RawTypeEdge, RUST_MACRO_NOISE};

pub(super) fn extract_rust(source: &str) -> (Vec<RawSymbol>, Vec<RawCallSite>, Vec<RawImport>, Vec<RawTypeEdge>) {
    let mut parser = match make_parser(tree_sitter_rust::language()) {
        Some(p) => p, None => return (vec![], vec![], vec![], vec![]),
    };
    let tree = match parser.parse(source.as_bytes(), None) {
        Some(t) => t, None => return (vec![], vec![], vec![], vec![]),
    };
    let mut syms = Vec::new();
    let mut calls = Vec::new();
    let mut imports = Vec::new();
    let mut type_edges = Vec::new();
    extract_rust_node(tree.root_node(), source.as_bytes(), &mut syms, &mut calls, &mut imports, &mut type_edges, "");
    (syms, calls, imports, type_edges)
}

fn extract_rust_node(
    node: tree_sitter::Node, src: &[u8],
    out: &mut Vec<RawSymbol>,
    calls: &mut Vec<RawCallSite>,
    imports: &mut Vec<RawImport>,
    type_edges: &mut Vec<RawTypeEdge>,
    context: &str,
) {
    let kind = node.kind();
    match kind {
        "function_item" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                let sig  = signature(node, src);
                let (return_type, params) = rust_fn_sig_parts(node, src);
                out.push(RawSymbol { name: name.clone(), kind: "fn".into(), line: node.start_position().row + 1, signature: sig, context: context.to_string(), return_type, params });
                let new_ctx = if context.is_empty() {
                    format!("fn {}", name)
                } else {
                    format!("{} · {}", context, name)
                };
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) {
                        if c.kind() == "block" {
                            extract_rust_node(c, src, out, calls, imports, type_edges, &new_ctx);
                        }
                    }
                }
                return;
            }
        }
        "struct_item" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name, kind: "struct".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string(), return_type: "".into(), params: "".into() });
            }
        }
        "enum_item" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name, kind: "enum".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string(), return_type: "".into(), params: "".into() });
            }
        }
        "trait_item" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                let ctx = format!("trait {}", name);
                out.push(RawSymbol { name: node_text(n, src).to_string(), kind: "trait".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string(), return_type: "".into(), params: "".into() });
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) { extract_rust_node(c, src, out, calls, imports, type_edges, &ctx); }
                }
                return;
            }
        }
        "impl_item" => {
            let impl_label = build_impl_label(node, src);
            // Extract type edge: `impl Trait for Type` → TypeEdge(Type → Trait, "implements")
            extract_rust_impl_type_edge(node, src, type_edges);
            for i in 0..node.child_count() {
                if let Some(c) = node.child(i) { extract_rust_node(c, src, out, calls, imports, type_edges, &impl_label); }
            }
            return;
        }
        "const_item" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name, kind: "const".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string(), return_type: "".into(), params: "".into() });
            }
        }
        "type_item" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name, kind: "type".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string(), return_type: "".into(), params: "".into() });
            }
        }
        "macro_definition" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name, kind: "macro".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string(), return_type: "".into(), params: "".into() });
            }
        }
        "call_expression" => {
            if let Some(cs) = parse_rust_call(node, src, context) {
                calls.push(cs);
            }
            for i in 0..node.child_count() {
                if let Some(c) = node.child(i) {
                    extract_rust_node(c, src, out, calls, imports, type_edges, context);
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
                    extract_rust_node(c, src, out, calls, imports, type_edges, context);
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
        if let Some(c) = node.child(i) { extract_rust_node(c, src, out, calls, imports, type_edges, context); }
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

/// Extract `(return_type, params_json)` from a `function_item` node.
fn rust_fn_sig_parts(node: tree_sitter::Node, src: &[u8]) -> (String, String) {
    let return_type = node.child_by_field_name("return_type")
        .map(|n| node_text(n, src).trim_start_matches("->").trim().to_string())
        .unwrap_or_default();

    let params_json = node.child_by_field_name("parameters")
        .map(|params_node| {
            let mut parts: Vec<&str> = Vec::new();
            for i in 0..params_node.child_count() {
                if let Some(c) = params_node.child(i) {
                    if matches!(c.kind(), "parameter" | "self_parameter") {
                        let text = std::str::from_utf8(&src[c.start_byte()..c.end_byte()]).unwrap_or("").trim();
                        if !text.is_empty() { parts.push(text); }
                    }
                }
            }
            params_to_json(&parts)
        })
        .unwrap_or_else(|| "[]".into());

    (return_type, params_json)
}

/// Emit a type edge for `impl Trait for Type` forms.
fn extract_rust_impl_type_edge(node: tree_sitter::Node, src: &[u8], type_edges: &mut Vec<RawTypeEdge>) {
    // `impl_item` structure: `impl` [trait_type `for`] type_name body
    // We want: impl Trait for Type  →  edge Type "implements" Trait
    let mut trait_name: Option<String> = None;
    let mut type_name: Option<String> = None;
    let mut saw_for = false;

    for i in 0..node.child_count() {
        let Some(c) = node.child(i) else { continue };
        match c.kind() {
            "for" => { saw_for = true; }
            "type_identifier" | "generic_type" | "scoped_type_identifier" => {
                let txt = node_text(c, src).to_string();
                if !saw_for {
                    trait_name = Some(txt);
                } else {
                    type_name = Some(txt);
                }
            }
            "declaration_list" => break,
            _ => {}
        }
    }

    if let (Some(trait_n), Some(type_n)) = (trait_name, type_name) {
        let base_trait = trait_n.split('<').next().unwrap_or(&trait_n).trim().to_string();
        if !base_trait.is_empty() && !type_n.is_empty() {
            type_edges.push(RawTypeEdge {
                child_name:  type_n,
                parent_name: base_trait,
                edge_kind:   "implements".into(),
                line:        node.start_position().row + 1,
            });
        }
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
