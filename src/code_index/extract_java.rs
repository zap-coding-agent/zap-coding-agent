use super::extract::{make_parser, node_text, signature, truncate_receiver, RawCallSite, RawImport, RawSymbol};

pub(super) fn extract_java(source: &str) -> (Vec<RawSymbol>, Vec<RawCallSite>, Vec<RawImport>) {
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
