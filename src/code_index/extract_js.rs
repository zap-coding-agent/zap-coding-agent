use super::extract::{make_parser, node_text, params_to_json, signature, truncate_receiver, RawCallSite, RawImport, RawSymbol, RawTypeEdge};

pub(super) fn extract_js(source: &str, typescript: bool, tsx: bool) -> (Vec<RawSymbol>, Vec<RawCallSite>, Vec<RawImport>, Vec<RawTypeEdge>) {
    let lang = if tsx {
        tree_sitter_typescript::language_tsx()
    } else if typescript {
        tree_sitter_typescript::language_typescript()
    } else {
        tree_sitter_javascript::language()
    };
    let mut parser = match make_parser(lang) { Some(p) => p, None => return (vec![], vec![], vec![], vec![]) };
    let tree = match parser.parse(source.as_bytes(), None) { Some(t) => t, None => return (vec![], vec![], vec![], vec![]) };
    let mut syms = Vec::new();
    let mut calls = Vec::new();
    let mut imports = Vec::new();
    let mut type_edges = Vec::new();
    extract_js_node(tree.root_node(), source.as_bytes(), &mut syms, &mut calls, &mut imports, &mut type_edges, "");
    (syms, calls, imports, type_edges)
}

fn extract_js_node(
    node: tree_sitter::Node, src: &[u8],
    out: &mut Vec<RawSymbol>,
    calls: &mut Vec<RawCallSite>,
    imports: &mut Vec<RawImport>,
    type_edges: &mut Vec<RawTypeEdge>,
    context: &str,
) {
    let kind = node.kind();
    match kind {
        "function_declaration" | "generator_function_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                let k = if kind == "generator_function_declaration" { "function*" } else { "function" };
                let (return_type, params) = js_fn_sig_parts(node, src);
                out.push(RawSymbol { name: name.clone(), kind: k.into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string(), return_type, params });
                let new_ctx = name.clone();
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) {
                        if c.kind() == "statement_block" { extract_js_node(c, src, out, calls, imports, type_edges, &new_ctx); }
                    }
                }
                return;
            }
        }
        "class_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name: name.clone(), kind: "class".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string(), return_type: "".into(), params: "".into() });
                extract_js_class_type_edges(node, src, &name, type_edges);
                let new_ctx = name.clone();
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) { extract_js_node(c, src, out, calls, imports, type_edges, &new_ctx); }
                }
                return;
            }
        }
        "method_definition" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                let (return_type, params) = js_fn_sig_parts(node, src);
                out.push(RawSymbol { name: name.clone(), kind: "method".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string(), return_type, params });
                let new_ctx = if context.is_empty() { name } else { format!("{}.{}", context, name) };
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) {
                        if c.kind() == "statement_block" { extract_js_node(c, src, out, calls, imports, type_edges, &new_ctx); }
                    }
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
                    if let Some(c) = node.child(i) { extract_js_node(c, src, out, calls, imports, type_edges, &new_ctx); }
                }
                return;
            }
        }
        "type_alias_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name, kind: "type".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string(), return_type: "".into(), params: "".into() });
            }
        }
        "enum_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name, kind: "enum".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string(), return_type: "".into(), params: "".into() });
            }
        }
        "lexical_declaration" | "variable_declaration" => {
            extract_js_var_decls(node, src, out, context);
            for i in 0..node.child_count() {
                if let Some(c) = node.child(i) {
                    extract_js_node(c, src, out, calls, imports, type_edges, context);
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
                    extract_js_node(c, src, out, calls, imports, type_edges, context);
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
        if let Some(c) = node.child(i) { extract_js_node(c, src, out, calls, imports, type_edges, context); }
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
                        let (return_type, params) = js_fn_sig_parts(val_node, src);
                        out.push(RawSymbol {
                            name, kind: k.into(),
                            line: decl.start_position().row + 1,
                            signature: signature(decl, src),
                            context: context.to_string(),
                            return_type, params,
                        });
                    }
                }
            }
        }
    }
}

fn js_fn_sig_parts(node: tree_sitter::Node, src: &[u8]) -> (String, String) {
    let return_type = node.child_by_field_name("return_type")
        .map(|n| node_text(n, src).trim_start_matches(':').trim().to_string())
        .unwrap_or_default();

    let params_json = node.child_by_field_name("parameters")
        .map(|params_node| {
            let mut parts: Vec<&str> = Vec::new();
            for i in 0..params_node.child_count() {
                if let Some(c) = params_node.child(i) {
                    if c.is_named() && !matches!(c.kind(), "(" | ")") {
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

fn extract_js_class_type_edges(node: tree_sitter::Node, src: &[u8], class_name: &str, type_edges: &mut Vec<RawTypeEdge>) {
    // `class_declaration` has `class_heritage` child which has `extends_clause` / `implements_clause`
    let line = node.start_position().row + 1;
    for i in 0..node.child_count() {
        let Some(c) = node.child(i) else { continue };
        if c.kind() == "class_heritage" {
            for j in 0..c.child_count() {
                let Some(clause) = c.child(j) else { continue };
                if clause.kind() == "extends_clause" {
                    if let Some(t) = clause.child_by_field_name("value") {
                        let parent = node_text(t, src).split('<').next().unwrap_or("").trim().to_string();
                        if !parent.is_empty() {
                            type_edges.push(RawTypeEdge { child_name: class_name.to_string(), parent_name: parent, edge_kind: "extends".into(), line });
                        }
                    }
                } else if clause.kind() == "implements_clause" {
                    for k in 0..clause.child_count() {
                        let Some(t) = clause.child(k) else { continue };
                        if t.kind() == "type_identifier" || t.kind() == "generic_type" {
                            let parent = node_text(t, src).split('<').next().unwrap_or("").trim().to_string();
                            if !parent.is_empty() {
                                type_edges.push(RawTypeEdge { child_name: class_name.to_string(), parent_name: parent, edge_kind: "implements".into(), line });
                            }
                        }
                    }
                }
            }
        }
    }
}

fn parse_js_call(node: tree_sitter::Node, src: &[u8], context: &str) -> Option<RawCallSite> {
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
                imports.push(RawImport {
                    line,
                    module: module.clone(),
                    imported_name: "default".into(),
                    alias: node_text(child, src).to_string(),
                });
            }
            "namespace_import" => {
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
