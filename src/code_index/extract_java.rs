use super::extract::{make_parser, node_text, params_to_json, signature, truncate_receiver, RawCallSite, RawImport, RawSymbol, RawTypeEdge};

pub(super) fn extract_java(source: &str) -> (Vec<RawSymbol>, Vec<RawCallSite>, Vec<RawImport>, Vec<RawTypeEdge>) {
    let mut parser = match make_parser(tree_sitter_java::language()) {
        Some(p) => p, None => return (vec![], vec![], vec![], vec![]),
    };
    let tree = match parser.parse(source.as_bytes(), None) {
        Some(t) => t, None => return (vec![], vec![], vec![], vec![]),
    };
    let mut syms = Vec::new();
    let mut calls = Vec::new();
    let mut imports = Vec::new();
    let mut type_edges = Vec::new();
    extract_java_node(tree.root_node(), source.as_bytes(), &mut syms, &mut calls, &mut imports, &mut type_edges, "");
    (syms, calls, imports, type_edges)
}

fn extract_java_node(
    node: tree_sitter::Node, src: &[u8],
    out: &mut Vec<RawSymbol>,
    calls: &mut Vec<RawCallSite>,
    imports: &mut Vec<RawImport>,
    type_edges: &mut Vec<RawTypeEdge>,
    context: &str,
) {
    let kind = node.kind();
    match kind {
        "class_declaration" | "enum_declaration" | "record_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                let k = match kind { "enum_declaration" => "enum", "record_declaration" => "record", _ => "class" };
                out.push(RawSymbol { name: name.clone(), kind: k.into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string(), return_type: "".into(), params: "".into() });
                extract_java_type_edges(node, src, &name, kind, type_edges);
                let new_ctx = name.clone();
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) { extract_java_node(c, src, out, calls, imports, type_edges, &new_ctx); }
                }
                return;
            }
        }
        "interface_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name: name.clone(), kind: "interface".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string(), return_type: "".into(), params: "".into() });
                let new_ctx = name.clone();
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) { extract_java_node(c, src, out, calls, imports, type_edges, &new_ctx); }
                }
                return;
            }
        }
        "method_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                let (return_type, params) = java_method_sig_parts(node, src);
                out.push(RawSymbol { name: name.clone(), kind: "method".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string(), return_type, params });
                let new_ctx = if context.is_empty() { name } else { format!("{} · {}", context, name) };
                if let Some(body) = node.child_by_field_name("body") {
                    extract_java_node(body, src, out, calls, imports, type_edges, &new_ctx);
                }
                return;
            }
        }
        "constructor_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                let (_, params) = java_method_sig_parts(node, src);
                out.push(RawSymbol { name: name.clone(), kind: "constructor".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string(), return_type: "".into(), params });
                let new_ctx = if context.is_empty() { name } else { format!("{} · ctor", context) };
                if let Some(body) = node.child_by_field_name("body") {
                    extract_java_node(body, src, out, calls, imports, type_edges, &new_ctx);
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
                                out.push(RawSymbol { name, kind: "const".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string(), return_type: "".into(), params: "".into() });
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
                    extract_java_node(c, src, out, calls, imports, type_edges, context);
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
                    extract_java_node(c, src, out, calls, imports, type_edges, context);
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
        if let Some(c) = node.child(i) { extract_java_node(c, src, out, calls, imports, type_edges, context); }
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

fn java_method_sig_parts(node: tree_sitter::Node, src: &[u8]) -> (String, String) {
    let return_type = node.child_by_field_name("type")
        .map(|n| node_text(n, src).to_string())
        .unwrap_or_default();

    let params_json = node.child_by_field_name("parameters")
        .map(|params_node| {
            let mut parts: Vec<&str> = Vec::new();
            for i in 0..params_node.child_count() {
                if let Some(c) = params_node.child(i) {
                    if matches!(c.kind(), "formal_parameter" | "spread_parameter") {
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

fn extract_java_type_edges(node: tree_sitter::Node, src: &[u8], class_name: &str, node_kind: &str, type_edges: &mut Vec<RawTypeEdge>) {
    let line = node.start_position().row + 1;
    for i in 0..node.child_count() {
        let Some(c) = node.child(i) else { continue };
        match c.kind() {
            "superclass" => {
                // `extends ClassName`
                if let Some(t) = (0..c.child_count()).filter_map(|j| c.child(j)).find(|n| matches!(n.kind(), "type_identifier" | "generic_type")) {
                    let parent = node_text(t, src).split('<').next().unwrap_or("").trim().to_string();
                    if !parent.is_empty() {
                        type_edges.push(RawTypeEdge { child_name: class_name.to_string(), parent_name: parent, edge_kind: "extends".into(), line });
                    }
                }
            }
            "super_interfaces" | "interface_type_list" => {
                // `implements InterfaceA, InterfaceB`
                for j in 0..c.child_count() {
                    let Some(t) = c.child(j) else { continue };
                    if matches!(t.kind(), "type_identifier" | "generic_type") {
                        let parent = node_text(t, src).split('<').next().unwrap_or("").trim().to_string();
                        if !parent.is_empty() {
                            let edge_kind = if node_kind == "interface_declaration" { "extends" } else { "implements" };
                            type_edges.push(RawTypeEdge { child_name: class_name.to_string(), parent_name: parent, edge_kind: edge_kind.into(), line });
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn flatten_java_import(node: tree_sitter::Node, src: &[u8], imports: &mut Vec<RawImport>) {
    let line = node.start_position().row + 1;
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
