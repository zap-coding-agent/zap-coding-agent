use super::extract::{make_parser, node_text, signature, truncate_receiver, RawCallSite, RawImport, RawSymbol};

pub(super) fn extract_python(source: &str) -> (Vec<RawSymbol>, Vec<RawCallSite>, Vec<RawImport>) {
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
