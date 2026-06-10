use super::extract::{make_parser, node_text, signature, truncate_receiver, RawCallSite, RawImport, RawSymbol};

pub(super) fn extract_go(source: &str) -> (Vec<RawSymbol>, Vec<RawCallSite>, Vec<RawImport>) {
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
            let operand = node.child_by_field_name("operand");
            let field = node.child_by_field_name("field")
                .map(|n| node_text(n, src).to_string()).unwrap_or_default();
            let op_text = operand.map(|n| node_text(n, src).to_string()).unwrap_or_default();
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
