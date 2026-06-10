use super::extract::{make_parser, node_text, signature, truncate_receiver, RawCallSite, RawImport, RawSymbol};

pub(crate) fn extract_csharp(source: &str) -> (Vec<RawSymbol>, Vec<RawCallSite>, Vec<RawImport>) {
    let mut parser = match make_parser(tree_sitter_c_sharp::language()) {
        Some(p) => p, None => return (vec![], vec![], vec![]),
    };
    let tree = match parser.parse(source.as_bytes(), None) {
        Some(t) => t, None => return (vec![], vec![], vec![]),
    };
    let mut syms = Vec::new();
    let mut calls = Vec::new();
    let mut imports = Vec::new();
    extract_csharp_node(tree.root_node(), source.as_bytes(), &mut syms, &mut calls, &mut imports, "");
    (syms, calls, imports)
}

fn extract_csharp_node(
    node: tree_sitter::Node, src: &[u8],
    out: &mut Vec<RawSymbol>,
    calls: &mut Vec<RawCallSite>,
    imports: &mut Vec<RawImport>,
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
                });
                let new_ctx = name.clone();
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) {
                        extract_csharp_node(c, src, out, calls, imports, &new_ctx);
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
                        extract_csharp_node(c, src, out, calls, imports, &new_ctx);
                    }
                }
                return;
            }
        }
        "method_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol {
                    name: name.clone(), kind: "method".into(),
                    line: node.start_position().row + 1,
                    signature: signature(node, src),
                    context: context.to_string(),
                });
                let new_ctx = if context.is_empty() { name } else { format!("{} · {}", context, name) };
                if let Some(body) = node.child_by_field_name("body") {
                    extract_csharp_node(body, src, out, calls, imports, &new_ctx);
                }
                return;
            }
        }
        "constructor_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol {
                    name: name.clone(), kind: "constructor".into(),
                    line: node.start_position().row + 1,
                    signature: signature(node, src),
                    context: context.to_string(),
                });
                let new_ctx = if context.is_empty() { name } else { format!("{} · ctor", context) };
                if let Some(body) = node.child_by_field_name("body") {
                    extract_csharp_node(body, src, out, calls, imports, &new_ctx);
                }
                return;
            }
        }
        "property_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol {
                    name, kind: "property".into(),
                    line: node.start_position().row + 1,
                    signature: signature(node, src),
                    context: context.to_string(),
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
                });
            }
        }
        "invocation_expression" => {
            if let Some(cs) = parse_csharp_invocation(node, src, context) {
                calls.push(cs);
            }
            for i in 0..node.child_count() {
                if let Some(c) = node.child(i) {
                    extract_csharp_node(c, src, out, calls, imports, context);
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
                    extract_csharp_node(c, src, out, calls, imports, context);
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
        if let Some(c) = node.child(i) { extract_csharp_node(c, src, out, calls, imports, context); }
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
