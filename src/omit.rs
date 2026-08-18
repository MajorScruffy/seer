use std::collections::HashMap;

use tree_sitter::Node;

use crate::ir::{CallKind, CallSite};
use crate::lang::rust::{node_text, path_segments};

const OMIT_MACROS: &[&str] = &["println", "eprintln", "print", "eprint", "dbg"];

#[derive(Clone, Debug, Default)]
pub struct UseMap {
    /// File-scoped `use` bindings: local name → path as written.
    bindings: HashMap<String, Vec<String>>,
    /// `use foo::*;` prefixes.
    globs: Vec<Vec<String>>,
}

enum Binding {
    Named { name: String, path: Vec<String> },
    Glob { prefix: Vec<String> },
}

impl UseMap {
    pub fn from_tree(root: Node, src: &str) -> Self {
        let mut map = Self::default();
        collect_use_decls(root, src, &mut map);
        map
    }

    /// Rewrite the first segment through file-scoped `use` bindings (and globs).
    pub fn canonicalize(&self, path: &[String]) -> Vec<Vec<String>> {
        if path.is_empty() {
            return Vec::new();
        }
        if let Some(bound) = self.bindings.get(&path[0]) {
            let mut out = bound.clone();
            out.extend(path.iter().skip(1).cloned());
            return vec![out];
        }
        let mut outs = vec![path.to_vec()];
        if path.len() == 1 {
            for glob in &self.globs {
                let mut p = glob.clone();
                p.extend(path.iter().cloned());
                outs.push(p);
            }
        }
        outs
    }
}

/// Steps 2–3 of the omit list. Step 1 (local never-omit) needs resolve.
pub fn should_omit(site: &CallSite, uses: &UseMap) -> bool {
    let path = match &site.kind {
        CallKind::Free { path } => path.as_slice(),
        CallKind::Method { .. } => return false,
    };
    if site.is_macro {
        if let Some(last) = path.last() {
            if OMIT_MACROS.contains(&last.as_str()) {
                return true;
            }
        }
    }
    uses.canonicalize(path)
        .into_iter()
        .any(|canon| matches!(canon.first().map(String::as_str), Some("log" | "tracing")))
}

fn collect_use_decls(node: Node, src: &str, map: &mut UseMap) {
    if node.kind() == "ERROR" {
        return;
    }
    if node.kind() == "use_declaration" {
        if let Some(arg) = node.child_by_field_name("argument") {
            for binding in parse_use_clause(arg, &[], src) {
                match binding {
                    Binding::Named { name, path } => {
                        map.bindings.insert(name, path);
                    }
                    Binding::Glob { prefix } => map.globs.push(prefix),
                }
            }
        }
        return;
    }
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i) {
            collect_use_decls(child, src, map);
        }
    }
}

fn parse_use_clause(node: Node, prefix: &[String], src: &str) -> Vec<Binding> {
    match node.kind() {
        "identifier" | "crate" | "self" | "super" | "metavariable" => {
            let seg = node_text(node, src);
            if seg == "self" && !prefix.is_empty() {
                return vec![Binding::Named {
                    name: prefix[prefix.len() - 1].clone(),
                    path: prefix.to_vec(),
                }];
            }
            let mut path = prefix.to_vec();
            path.push(seg.to_string());
            let name = path[path.len() - 1].clone();
            vec![Binding::Named { name, path }]
        }
        "scoped_identifier" => {
            let mut path = prefix.to_vec();
            path.extend(path_segments(node, src));
            if path.is_empty() {
                return Vec::new();
            }
            let name = path[path.len() - 1].clone();
            vec![Binding::Named { name, path }]
        }
        "use_as_clause" => {
            let Some(path_node) = node.child_by_field_name("path") else {
                return Vec::new();
            };
            let Some(alias) = node.child_by_field_name("alias") else {
                return Vec::new();
            };
            let mut path = prefix.to_vec();
            if node_text(path_node, src) == "self" && !prefix.is_empty() {
                // `use foo::{self as baz}` binds baz → ["foo"]
            } else {
                path.extend(path_segments(path_node, src));
            }
            vec![Binding::Named {
                name: node_text(alias, src).to_string(),
                path,
            }]
        }
        "use_list" => {
            let mut out = Vec::new();
            for i in 0..node.named_child_count() {
                if let Some(child) = node.named_child(i) {
                    out.extend(parse_use_clause(child, prefix, src));
                }
            }
            out
        }
        "scoped_use_list" => {
            let mut new_prefix = prefix.to_vec();
            if let Some(p) = node.child_by_field_name("path") {
                new_prefix.extend(path_segments(p, src));
            }
            node.child_by_field_name("list")
                .map(|list| parse_use_clause(list, &new_prefix, src))
                .unwrap_or_default()
        }
        "use_wildcard" => {
            let mut glob_prefix = prefix.to_vec();
            if let Some(p) = node.named_child(0) {
                glob_prefix.extend(path_segments(p, src));
            }
            vec![Binding::Glob {
                prefix: glob_prefix,
            }]
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::{extract_fn, find_function_item, print_raw_fn};
    use crate::parse::parse_rust;

    fn outline_of(src: &str, name: &str) -> String {
        let tree = parse_rust(src);
        let uses = UseMap::from_tree(tree.root_node(), src);
        let fn_item = find_function_item(tree.root_node(), src, name).expect(name);
        let body = extract_fn(fn_item, src, "input.rs", &uses);
        print_raw_fn(name, &body)
    }

    fn free_macro(path: &[&str]) -> CallSite {
        CallSite {
            display: String::new(),
            kind: CallKind::Free {
                path: path.iter().map(|s| (*s).to_string()).collect(),
            },
            is_macro: true,
            file: "input.rs".into(),
        }
    }

    fn free_call(path: &[&str]) -> CallSite {
        CallSite {
            display: String::new(),
            kind: CallKind::Free {
                path: path.iter().map(|s| (*s).to_string()).collect(),
            },
            is_macro: false,
            file: "input.rs".into(),
        }
    }

    #[test]
    fn omit_each_rust_name() {
        let uses = UseMap::default();
        for name in OMIT_MACROS {
            assert!(
                should_omit(&free_macro(&[name]), &uses),
                "{name}! should be omitted"
            );
        }
        assert!(should_omit(&free_macro(&["log", "warn"]), &uses));
        assert!(should_omit(&free_call(&["log", "warn"]), &uses));
        assert!(should_omit(&free_macro(&["tracing", "info"]), &uses));
        let src = r#"
fn f() {
    println!("x");
    eprintln!("x");
    print!("x");
    eprint!("x");
    dbg!(1);
    log::warn!("empty");
    tracing::info!("t");
    log::warn("empty");
    return;
}
"#;
        assert_eq!(outline_of(src, "f"), "fn f\n  return\n");
    }

    #[test]
    fn omit_keeps_format() {
        let uses = UseMap::default();
        assert!(!should_omit(&free_macro(&["format"]), &uses));
        assert!(!should_omit(&free_macro(&["vec"]), &uses));
        assert!(!should_omit(&free_macro(&["todo"]), &uses));
        assert!(!should_omit(&free_macro(&["assert"]), &uses));
        assert!(!should_omit(
            &CallSite {
                display: String::new(),
                kind: CallKind::Method {
                    name: "unwrap".into()
                },
                is_macro: false,
                file: "input.rs".into(),
            },
            &uses
        ));
        let src = r#"
fn f() {
    format!("a{}", 1);
}
"#;
        assert_eq!(outline_of(src, "f"), "fn f\n  format!(\"a{}\", 1)\n");
    }

    #[test]
    fn omit_keeps_local_info() {
        let src = r#"
fn info() {
    return;
}
fn f() {
    info();
}
"#;
        assert_eq!(outline_of(src, "f"), "fn f\n  info()\n");
    }

    #[test]
    fn omit_use_log_warn_macro() {
        let src = r#"
use log::warn;
fn f() {
    warn!("x");
    return;
}
"#;
        assert_eq!(outline_of(src, "f"), "fn f\n  return\n");
    }
}
