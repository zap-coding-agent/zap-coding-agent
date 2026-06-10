use super::extract::{make_parser, node_text, params_to_json, signature, truncate_receiver, RawCallSite, RawImport, RawSymbol, RawTypeEdge};

pub(crate) fn extract_csharp(source: &str) -> (Vec<RawSymbol>, Vec<RawCallSite>, Vec<RawImport>, Vec<RawTypeEdge>) {
    let mut parser = match make_parser(tree_sitter_c_sharp::language()) {
        Some(p) => p, None => return (vec![], vec![], vec![], vec![]),
    };
    let tree = match parser.parse(source.as_bytes(), None) {
        Some(t) => t, None => return (vec![], vec![], vec![], vec![]),
    };
    let mut syms = Vec::new();
    let mut calls = Vec::new();
    let mut imports = Vec::new();
    let mut type_edges = Vec::new();
    extract_csharp_node(tree.root_node(), source.as_bytes(), &mut syms, &mut calls, &mut imports, &mut type_edges, "");
    (syms, calls, imports, type_edges)
}

fn extract_csharp_node(
    node: tree_sitter::Node, src: &[u8],
    out: &mut Vec<RawSymbol>,
    calls: &mut Vec<RawCallSite>,
    imports: &mut Vec<RawImport>,
    type_edges: &mut Vec<RawTypeEdge>,
    context: &str,
) {
    let kind = node.kind();
    match kind {
        "class_declaration" | "struct_declaration" | "interface_declaration"
        | "enum_declaration" | "record_declaration" | "record_struct_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                let k = match kind {
                    "struct_declaration" => "struct",
                    "interface_declaration" => "interface",
                    "enum_declaration" => "enum",
                    "record_declaration" => "record",
                    "record_struct_declaration" => "record struct",
                    _ => "class",
                };
                out.push(RawSymbol {
                    name: name.clone(), kind: k.into(),
                    line: node.start_position().row + 1,
                    signature: signature(node, src),
                    context: context.to_string(),
                    return_type: "".into(), params: "".into(),
                });
                extract_csharp_type_edges(node, src, &name, type_edges);
                let new_ctx = name.clone();
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) {
                        extract_csharp_node(c, src, out, calls, imports, type_edges, &new_ctx);
                    }
                }
                return;
            }
        }
        "namespace_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                let new_ctx = if context.is_empty() { name } else { format!("{}.{}", context, name) };
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) {
                        extract_csharp_node(c, src, out, calls, imports, type_edges, &new_ctx);
                    }
                }
                return;
            }
        }
        "method_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                let (return_type, params) = csharp_method_sig_parts(node, src);
                out.push(RawSymbol {
                    name: name.clone(), kind: "method".into(),
                    line: node.start_position().row + 1,
                    signature: signature(node, src),
                    context: context.to_string(),
                    return_type, params,
                });
                let new_ctx = if context.is_empty() { name } else { format!("{} · {}", context, name) };
                if let Some(body) = node.child_by_field_name("body") {
                    extract_csharp_node(body, src, out, calls, imports, type_edges, &new_ctx);
                }
                return;
            }
        }
        "constructor_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                let (_, params) = csharp_method_sig_parts(node, src);
                out.push(RawSymbol {
                    name: name.clone(), kind: "constructor".into(),
                    line: node.start_position().row + 1,
                    signature: signature(node, src),
                    context: context.to_string(),
                    return_type: "".into(), params,
                });
                let new_ctx = if context.is_empty() { name } else { format!("{} · ctor", context) };
                if let Some(body) = node.child_by_field_name("body") {
                    extract_csharp_node(body, src, out, calls, imports, type_edges, &new_ctx);
                }
                return;
            }
        }
        "property_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                let return_type = node.child_by_field_name("type")
                    .map(|t| node_text(t, src).to_string()).unwrap_or_default();
                out.push(RawSymbol {
                    name, kind: "property".into(),
                    line: node.start_position().row + 1,
                    signature: signature(node, src),
                    context: context.to_string(),
                    return_type, params: "".into(),
                });
            }
        }
        "field_declaration" => {
            let text = node_text(node, src);
            if text.contains("const") || (text.contains("static") && text.contains("readonly")) {
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) {
                        if c.kind() == "variable_declaration" {
                            if let Some(n) = c.child_by_field_name("name") {
                                let name = node_text(n, src).to_string();
                                out.push(RawSymbol {
                                    name, kind: "const".into(),
                                    line: node.start_position().row + 1,
                                    signature: signature(node, src),
                                    context: context.to_string(),
                                    return_type: "".into(), params: "".into(),
                                });
                            }
                        }
                    }
                }
            }
        }
        "delegate_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol {
                    name, kind: "delegate".into(),
                    line: node.start_position().row + 1,
                    signature: signature(node, src),
                    context: context.to_string(),
                    return_type: "".into(), params: "".into(),
                });
            }
        }
        "invocation_expression" => {
            if let Some(cs) = parse_csharp_invocation(node, src, context) {
                calls.push(cs);
            }
            for i in 0..node.child_count() {
                if let Some(c) = node.child(i) {
                    extract_csharp_node(c, src, out, calls, imports, type_edges, context);
                }
            }
            return;
        }
        "object_creation_expression" => {
            if let Some(cs) = parse_csharp_object_creation(node, src, context) {
                calls.push(cs);
            }
            for i in 0..node.child_count() {
                if let Some(c) = node.child(i) {
                    extract_csharp_node(c, src, out, calls, imports, type_edges, context);
                }
            }
            return;
        }
        "using_directive" => {
            flatten_csharp_using(node, src, imports);
            return;
        }
        _ => {}
    }
    for i in 0..node.child_count() {
        if let Some(c) = node.child(i) { extract_csharp_node(c, src, out, calls, imports, type_edges, context); }
    }
}

fn parse_csharp_invocation(node: tree_sitter::Node, src: &[u8], context: &str) -> Option<RawCallSite> {
    let function = node.child_by_field_name("function")?;
    let (qualifier, name, receiver) = unwrap_csharp_callable(function, src);
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

fn unwrap_csharp_callable(node: tree_sitter::Node, src: &[u8]) -> (String, String, String) {
    match node.kind() {
        "identifier" => ("".into(), node_text(node, src).to_string(), "".into()),
        "member_access_expression" => {
            let expr = node.child_by_field_name("expression");
            let name = node.child_by_field_name("name")
                .map(|n| node_text(n, src).to_string()).unwrap_or_default();
            let expr_text = expr.map(|n| node_text(n, src).to_string()).unwrap_or_default();
            let looks_like_path = !expr_text.is_empty()
                && expr_text.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
            if looks_like_path {
                (expr_text, name, "".into())
            } else {
                ("".into(), name, truncate_receiver(&expr_text))
            }
        }
        _ => ("".into(), "".into(), "".into()),
    }
}

fn parse_csharp_object_creation(node: tree_sitter::Node, src: &[u8], context: &str) -> Option<RawCallSite> {
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

fn csharp_method_sig_parts(node: tree_sitter::Node, src: &[u8]) -> (String, String) {
    let return_type = node.child_by_field_name("type")
        .map(|n| node_text(n, src).to_string())
        .unwrap_or_default();

    let params_json = node.child_by_field_name("parameters")
        .map(|params_node| {
            let mut parts: Vec<&str> = Vec::new();
            for i in 0..params_node.child_count() {
                if let Some(c) = params_node.child(i) {
                    if matches!(c.kind(), "parameter") {
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

fn extract_csharp_type_edges(node: tree_sitter::Node, src: &[u8], class_name: &str, type_edges: &mut Vec<RawTypeEdge>) {
    // `base_list` child: contains base type + interfaces separated by commas
    let line = node.start_position().row + 1;
    if let Some(base_list) = (0..node.child_count())
        .filter_map(|i| node.child(i))
        .find(|c| c.kind() == "base_list")
    {
        let mut first = true;
        for i in 0..base_list.child_count() {
            let Some(c) = base_list.child(i) else { continue };
            if matches!(c.kind(), "identifier" | "qualified_name" | "generic_name") {
                let parent = node_text(c, src).split('<').next().unwrap_or("").trim().to_string();
                if parent.is_empty() { first = false; continue; }
                let edge_kind = if first { "extends" } else { "implements" };
                type_edges.push(RawTypeEdge { child_name: class_name.to_string(), parent_name: parent, edge_kind: edge_kind.into(), line });
                first = false;
            }
        }
    }
}

fn flatten_csharp_using(node: tree_sitter::Node, src: &[u8], imports: &mut Vec<RawImport>) {
    let line = node.start_position().row + 1;
    let name_node = node.child_by_field_name("name")
        .or_else(|| (0..node.child_count())
            .filter_map(|i| node.child(i))
            .find(|c| matches!(c.kind(), "qualified_name" | "identifier")));
    if let Some(n) = name_node {
        let module = node_text(n, src).to_string();
        if !module.is_empty() {
            imports.push(RawImport { line, module, imported_name: "".into(), alias: "".into() });
        }
    }
}
