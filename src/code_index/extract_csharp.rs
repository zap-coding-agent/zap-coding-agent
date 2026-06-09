use super::extract::{make_parser,node_text,signature,RawSymbol};

pub(crate) fn extract_csharp(source: &str) -> Vec<RawSymbol> {
    let mut parser = match make_parser(tree_sitter_c_sharp::language()) {
        Some(p) => p, None => return vec![],
    };
    let tree = match parser.parse(source.as_bytes(), None) {
        Some(t) => t, None => return vec![],
    };
    let mut out = Vec::new();
    extract_csharp_node(tree.root_node(), source.as_bytes(), &mut out, "");
    out
}

fn extract_csharp_node(node: tree_sitter::Node, src: &[u8], out: &mut Vec<RawSymbol>, context: &str) {
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
                        extract_csharp_node(c, src, out, &new_ctx);
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
                        extract_csharp_node(c, src, out, &new_ctx);
                    }
                }
                return;
            }
        }
        "method_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol {
                    name, kind: "method".into(),
                    line: node.start_position().row + 1,
                    signature: signature(node, src),
                    context: context.to_string(),
                });
                return;
            }
        }
        "constructor_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol {
                    name, kind: "constructor".into(),
                    line: node.start_position().row + 1,
                    signature: signature(node, src),
                    context: context.to_string(),
                });
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
        _ => {}
    }
    for i in 0..node.child_count() {
        if let Some(c) = node.child(i) { extract_csharp_node(c, src, out, context); }
    }
}
