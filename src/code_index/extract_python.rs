use super::extract::{make_parser, node_text, params_to_json, signature, truncate_receiver, RawCallSite, RawImport, RawSymbol, RawTypeEdge};

pub(super) fn extract_python(source: &str) -> (Vec<RawSymbol>, Vec<RawCallSite>, Vec<RawImport>, Vec<RawTypeEdge>) {
    let mut parser = match make_parser(tree_sitter_python::language()) {
        Some(p) => p, None => return (vec![], vec![], vec![], vec![]),
    };
    let tree = match parser.parse(source.as_bytes(), None) {
        Some(t) => t, None => return (vec![], vec![], vec![], vec![]),
    };
    let mut syms = Vec::new();
    let mut calls = Vec::new();
    let mut imports = Vec::new();
    let mut type_edges = Vec::new();
    extract_python_node(tree.root_node(), source.as_bytes(), &mut syms, &mut calls, &mut imports, &mut type_edges, "");
    (syms, calls, imports, type_edges)
}

fn extract_python_node(
    node: tree_sitter::Node, src: &[u8],
    out: &mut Vec<RawSymbol>,
    calls: &mut Vec<RawCallSite>,
    imports: &mut Vec<RawImport>,
    type_edges: &mut Vec<RawTypeEdge>,
    context: &str,
) {
    let kind = node.kind();
    match kind {
        "function_definition" | "async_function_definition" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                let k = if kind == "async_function_definition" { "async fn" } else { "def" };
                let (return_type, params) = python_fn_sig_parts(node, src);
                out.push(RawSymbol { name: name.clone(), kind: k.into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string(), return_type, params });
                let new_ctx = if context.is_empty() { name.clone() } else { format!("{}.{}", context, name) };
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) {
                        if c.kind() == "block" { extract_python_node(c, src, out, calls, imports, type_edges, &new_ctx); }
                    }
                }
                return;
            }
        }
        "class_definition" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name: name.clone(), kind: "class".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string(), return_type: "".into(), params: "".into() });
                extract_python_class_type_edges(node, src, &name, type_edges);
                let new_ctx = if context.is_empty() { name.clone() } else { format!("{}.{}", context, name) };
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) { extract_python_node(c, src, out, calls, imports, type_edges, &new_ctx); }
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
                    extract_python_node(c, src, out, calls, imports, type_edges, context);
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
        if let Some(c) = node.child(i) { extract_python_node(c, src, out, calls, imports, type_edges, context); }
    }
}

fn python_fn_sig_parts(node: tree_sitter::Node, src: &[u8]) -> (String, String) {
    let return_type = node.child_by_field_name("return_type")
        .map(|n| node_text(n, src).trim_start_matches("->").trim().to_string())
        .unwrap_or_default();

    let params_json = node.child_by_field_name("parameters")
        .map(|params_node| {
            let mut parts: Vec<&str> = Vec::new();
            for i in 0..params_node.child_count() {
                if let Some(c) = params_node.child(i) {
                    if c.is_named() && !matches!(c.kind(), "(" | ")") {
                        let text = std::str::from_utf8(&src[c.start_byte()..c.end_byte()]).unwrap_or("").trim();
                        if !text.is_empty() && text != "self" && text != "cls" { parts.push(text); }
                    }
                }
            }
            params_to_json(&parts)
        })
        .unwrap_or_else(|| "[]".into());

    (return_type, params_json)
}

fn extract_python_class_type_edges(node: tree_sitter::Node, src: &[u8], class_name: &str, type_edges: &mut Vec<RawTypeEdge>) {
    // `class_definition` has an `superclasses` field (argument_list node)
    let line = node.start_position().row + 1;
    if let Some(supers) = node.child_by_field_name("superclasses") {
        for i in 0..supers.child_count() {
            if let Some(c) = supers.child(i) {
                if c.is_named() {
                    let parent = node_text(c, src).trim().to_string();
                    // Skip keyword-only nodes or empty
                    if parent.is_empty() || parent == "," { continue; }
                    // Strip generic params: `Generic[T]` → `Generic`
                    let base = parent.split('[').next().unwrap_or(&parent).trim().to_string();
                    if !base.is_empty() {
                        type_edges.push(RawTypeEdge {
                            child_name:  class_name.to_string(),
                            parent_name: base,
                            edge_kind:   "extends".into(),
                            line,
                        });
                    }
                }
            }
        }
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
            let obj = node.child_by_field_name("object");
            let attr = node.child_by_field_name("attribute")
                .map(|n| node_text(n, src).to_string()).unwrap_or_default();
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
        for i in 0..node.child_count() {
            if let Some(c) = node.child(i) {
                match c.kind() {
                    "dotted_name" | "identifier" => {
                        let text = node_text(c, src).to_string();
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
