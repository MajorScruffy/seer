use std::collections::{BTreeMap, HashSet};

use crate::ir::{FnId, RawNode};
use crate::omit::{should_omit_resolved, UseMap};
use crate::resolve::{called_functions, index_files, resolve, select_entries, ResolveIndex};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GraphNode {
    Function {
        id: String,
        name: String,
        file: String,
        line: usize,
        entry: bool,
        start_byte: usize,
        end_byte: usize,
    },
    Leaf {
        id: String,
        label: String,
    },
}

impl GraphNode {
    fn id(&self) -> &str {
        match self {
            Self::Function { id, .. } | Self::Leaf { id, .. } => id,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub label: String,
    /// 0-based index of this kept call in the enclosing function body.
    pub seq: usize,
}

fn function_node_id(id: &FnId) -> String {
    format!("{}:{}", id.file, id.start_byte)
}

/// Same collect contract as `outline_files`: `(posix_relpath, source_utf8)`.
pub fn graph_files(files: &[(String, String)]) -> CallGraph {
    build_call_graph(&index_files(files))
}

fn build_call_graph(index: &ResolveIndex) -> CallGraph {
    let entries: HashSet<FnId> = select_entries(index, &called_functions(index))
        .into_iter()
        .collect();
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut seen_ids = HashSet::new();

    for def in &index.defs {
        if !def.has_body {
            continue;
        }
        let id = function_node_id(&def.id);
        nodes.push(GraphNode::Function {
            id: id.clone(),
            name: def.name.clone(),
            file: def.id.file.clone(),
            line: def.start_line,
            entry: entries.contains(&def.id),
            start_byte: def.id.start_byte,
            end_byte: def.end_byte,
        });
        seen_ids.insert(id);
    }

    for def in &index.defs {
        if !def.has_body {
            continue;
        }
        let from = function_node_id(&def.id);
        let mut seq = 0;
        collect_call_edges(
            &def.body,
            &from,
            index,
            &mut nodes,
            &mut edges,
            &mut seen_ids,
            &mut seq,
        );
    }

    sort_graph(&mut nodes, &mut edges);
    CallGraph { nodes, edges }
}

fn collect_call_edges(
    raws: &[RawNode],
    from: &str,
    index: &ResolveIndex,
    nodes: &mut Vec<GraphNode>,
    edges: &mut Vec<GraphEdge>,
    seen_ids: &mut HashSet<String>,
    seq: &mut usize,
) {
    let empty = UseMap::default();
    for raw in raws {
        match raw {
            RawNode::Control { children, .. } => {
                collect_call_edges(children, from, index, nodes, edges, seen_ids, seq);
            }
            RawNode::NestedFn { .. } => {}
            RawNode::Call { site } => {
                let uses = index.uses.get(&site.file).unwrap_or(&empty);
                let target = resolve(site, index);
                if should_omit_resolved(site, uses, target.is_some()) {
                    continue;
                }
                let to = match target {
                    Some(id) => function_node_id(&id),
                    None => {
                        let leaf = format!("leaf:{}", site.display);
                        if seen_ids.insert(leaf.clone()) {
                            nodes.push(GraphNode::Leaf {
                                id: leaf.clone(),
                                label: site.display.clone(),
                            });
                        }
                        leaf
                    }
                };
                edges.push(GraphEdge {
                    from: from.to_string(),
                    to,
                    label: site.display.clone(),
                    seq: *seq,
                });
                *seq += 1;
            }
        }
    }
}

fn sort_graph(nodes: &mut [GraphNode], edges: &mut [GraphEdge]) {
    nodes.sort_by(|a, b| match (a, b) {
        (GraphNode::Function { .. }, GraphNode::Leaf { .. }) => std::cmp::Ordering::Less,
        (GraphNode::Leaf { .. }, GraphNode::Function { .. }) => std::cmp::Ordering::Greater,
        (
            GraphNode::Function {
                file: fa,
                start_byte: sa,
                ..
            },
            GraphNode::Function {
                file: fb,
                start_byte: sb,
                ..
            },
        ) => fa.cmp(fb).then(sa.cmp(sb)),
        (GraphNode::Leaf { id: a, .. }, GraphNode::Leaf { id: b, .. }) => a.cmp(b),
    });
    edges.sort_by(|a, b| {
        a.from
            .cmp(&b.from)
            .then(a.to.cmp(&b.to))
            .then(a.label.cmp(&b.label))
            .then(a.seq.cmp(&b.seq))
    });
}

pub fn print_graph_mermaid(graph: &CallGraph) -> String {
    if graph.nodes.is_empty() {
        return String::new();
    }
    let mut out = String::from("flowchart TD\n");
    for (i, node) in graph.nodes.iter().enumerate() {
        let label = match node {
            GraphNode::Function {
                file, line, name, ..
            } => format!("{file}:{line} {name}"),
            GraphNode::Leaf { label, .. } => label.clone(),
        };
        let label = label.replace('"', "'");
        out.push_str("  n");
        out.push_str(&i.to_string());
        out.push_str("[\"");
        out.push_str(&label);
        out.push_str("\"]\n");
    }
    let mut seen_pairs = HashSet::new();
    for edge in &graph.edges {
        if !seen_pairs.insert((edge.from.as_str(), edge.to.as_str())) {
            continue;
        }
        let from = node_index(&graph.nodes, &edge.from);
        let to = node_index(&graph.nodes, &edge.to);
        out.push_str("  n");
        out.push_str(&from.to_string());
        out.push_str(" --> n");
        out.push_str(&to.to_string());
        out.push('\n');
    }
    out
}

fn node_index(nodes: &[GraphNode], id: &str) -> usize {
    nodes
        .iter()
        .position(|n| n.id() == id)
        .expect("edge endpoint is a graph node")
}

pub fn print_graph_json(graph: &CallGraph) -> String {
    if graph.nodes.is_empty() {
        return String::new();
    }
    let mut out = String::from("{\n  \"nodes\": [\n");
    for (i, node) in graph.nodes.iter().enumerate() {
        if i > 0 {
            out.push_str(",\n");
        }
        match node {
            GraphNode::Function {
                id,
                name,
                file,
                line,
                entry,
                ..
            } => {
                out.push_str("    {\n      \"id\": ");
                write_json_string(&mut out, id);
                out.push_str(",\n      \"kind\": \"function\",\n      \"name\": ");
                write_json_string(&mut out, name);
                out.push_str(",\n      \"file\": ");
                write_json_string(&mut out, file);
                out.push_str(",\n      \"line\": ");
                out.push_str(&line.to_string());
                out.push_str(",\n      \"entry\": ");
                out.push_str(if *entry { "true" } else { "false" });
                out.push_str("\n    }");
            }
            GraphNode::Leaf { id, label } => {
                out.push_str("    {\n      \"id\": ");
                write_json_string(&mut out, id);
                out.push_str(",\n      \"kind\": \"leaf\",\n      \"label\": ");
                write_json_string(&mut out, label);
                out.push_str("\n    }");
            }
        }
    }
    out.push_str("\n  ],\n  \"edges\": [");
    if graph.edges.is_empty() {
        out.push_str("]\n}\n");
        return out;
    }
    out.push('\n');
    for (i, edge) in graph.edges.iter().enumerate() {
        if i > 0 {
            out.push_str(",\n");
        }
        out.push_str("    {\n      \"from\": ");
        write_json_string(&mut out, &edge.from);
        out.push_str(",\n      \"to\": ");
        write_json_string(&mut out, &edge.to);
        out.push_str(",\n      \"label\": ");
        write_json_string(&mut out, &edge.label);
        out.push_str(",\n      \"seq\": ");
        out.push_str(&edge.seq.to_string());
        out.push_str("\n    }");
    }
    out.push_str("\n  ]\n}\n");
    out
}

fn write_json_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

pub fn print_graph_html(graph: &CallGraph, files: &[(String, String)]) -> String {
    if graph.nodes.is_empty() {
        return String::new();
    }
    let json = print_graph_json(graph).replace('<', "\\u003c");
    let sources = print_source_payload(graph, files).replace('<', "\\u003c");
    let css = include_str!("graph.css");
    let js = include_str!("graph.js");
    let mut out = String::from(
        "<!DOCTYPE html>\n\
<html lang=\"en\">\n\
<head>\n\
<meta charset=\"utf-8\">\n\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
<title>seer graph</title>\n\
<style>\n",
    );
    out.push_str(css);
    out.push_str(
        "</style>\n\
</head>\n\
<body>\n\
<aside id=\"panel\">\n\
  <h1>seer graph</h1>\n\
  <p id=\"hint\">A box is a function; boxes inside it are the functions it calls, left to right. Nested calls start expanded; the triangle collapses a callee. A dashed box with ↩ is a recursive call back to a function already on this path. Drag to pan vertically. Scroll, +/−, or the buttons to zoom.</p>\n\
  <label>reachable from\n\
    <select id=\"entry\"><option value=\"\">all</option></select>\n\
  </label>\n\
  <p id=\"detail\">click a node</p>\n\
  <pre id=\"code\"></pre>\n\
</aside>\n\
<div id=\"stage\">\n\
<canvas id=\"canvas\" tabindex=\"0\"></canvas>\n\
<div id=\"zoom-bar\">\n\
  <button type=\"button\" id=\"zoom-out\" title=\"Zoom out\">−</button>\n\
  <button type=\"button\" id=\"zoom-fit\" title=\"Fit width\">fit</button>\n\
  <button type=\"button\" id=\"zoom-in\" title=\"Zoom in\">+</button>\n\
</div>\n\
</div>\n\
<script type=\"application/json\" id=\"seer-graph\">\n",
    );
    out.push_str(&json);
    out.push_str("</script>\n<script type=\"application/json\" id=\"seer-sources\">\n");
    out.push_str(&sources);
    out.push_str("</script>\n<script>\n");
    out.push_str(js);
    out.push_str("</script>\n</body>\n</html>\n");
    out
}

fn print_source_payload(graph: &CallGraph, files: &[(String, String)]) -> String {
    let mut wanted = BTreeMap::new();
    for node in &graph.nodes {
        if let GraphNode::Function { file, .. } = node {
            if !wanted.contains_key(file) {
                if let Some((_, src)) = files.iter().find(|(p, _)| p == file) {
                    wanted.insert(file.clone(), src.as_str());
                }
            }
        }
    }
    let mut out = String::from("{\n  \"files\": {");
    if wanted.is_empty() {
        out.push_str("},\n  \"spans\": {}\n}\n");
        return out;
    }
    out.push('\n');
    for (i, (path, src)) in wanted.iter().enumerate() {
        if i > 0 {
            out.push_str(",\n");
        }
        out.push_str("    ");
        write_json_string(&mut out, path);
        out.push_str(": ");
        write_json_string(&mut out, src);
    }
    out.push_str("\n  },\n  \"spans\": {");
    let mut first_span = true;
    for node in &graph.nodes {
        if let GraphNode::Function {
            id,
            start_byte,
            end_byte,
            ..
        } = node
        {
            if !first_span {
                out.push(',');
            } else {
                first_span = false;
            }
            out.push_str("\n    ");
            write_json_string(&mut out, id);
            out.push_str(": [");
            out.push_str(&start_byte.to_string());
            out.push_str(", ");
            out.push_str(&end_byte.to_string());
            out.push(']');
        }
    }
    out.push_str("\n  }\n}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn html_seer_graph_json(html: &str) -> &str {
        let start = html
            .find("<script type=\"application/json\" id=\"seer-graph\">")
            .expect("seer-graph script");
        let after = html[start..]
            .find('\n')
            .map(|i| start + i + 1)
            .expect("script newline");
        let end = html[after..]
            .find("</script>")
            .map(|i| after + i)
            .expect("script close");
        &html[after..end]
    }

    fn graph(src: &str) -> CallGraph {
        graph_files(&[("input.rs".into(), src.into())])
    }

    fn fn_named<'a>(g: &'a CallGraph, name: &str) -> &'a GraphNode {
        g.nodes
            .iter()
            .find(|n| matches!(n, GraphNode::Function { name: n, .. } if n == name))
            .unwrap_or_else(|| panic!("missing function {name}"))
    }

    fn fn_id<'a>(g: &'a CallGraph, name: &str) -> &'a str {
        match fn_named(g, name) {
            GraphNode::Function { id, .. } => id,
            GraphNode::Leaf { .. } => unreachable!(),
        }
    }

    #[test]
    fn graph_diamond_shares_d() {
        let g =
            graph("fn a() { b(); c(); }\nfn b() { d(); }\nfn c() { d(); }\nfn d() { return; }\n");
        let ds: Vec<_> = g
            .nodes
            .iter()
            .filter(|n| matches!(n, GraphNode::Function { name, .. } if name == "d"))
            .collect();
        assert_eq!(ds.len(), 1);
        let d = fn_id(&g, "d");
        let incoming = g.edges.iter().filter(|e| e.to == d).count();
        assert_eq!(incoming, 2);
    }

    #[test]
    fn graph_recursive_self_edge() {
        let g = graph("fn walk() { walk(); }\n");
        assert_eq!(g.nodes.len(), 1);
        assert_eq!(g.edges.len(), 1);
        assert_eq!(g.edges[0].from, g.edges[0].to);
    }

    #[test]
    fn graph_omit_println() {
        let g = graph("fn f() { println!(\"x\"); }\n");
        assert!(g.edges.is_empty());
        assert!(g
            .nodes
            .iter()
            .all(|n| matches!(n, GraphNode::Function { .. })));
    }

    #[test]
    fn graph_empty_print() {
        let g = graph("");
        assert_eq!(print_graph_mermaid(&g), "");
        assert_eq!(print_graph_json(&g), "");
        assert_eq!(print_graph_html(&g, &[]), "");
    }

    #[test]
    fn graph_mermaid_coalesces_duplicate_edges() {
        let g =
            graph("fn f() { let x = compute(); let y = compute(); }\nfn compute() { return; }\n");
        let mmd = print_graph_mermaid(&g);
        let arrows: Vec<_> = mmd.lines().filter(|l| l.contains("-->")).collect();
        assert_eq!(arrows.len(), 1);
        let f = node_index(&g.nodes, fn_id(&g, "f"));
        let c = node_index(&g.nodes, fn_id(&g, "compute"));
        assert_eq!(arrows[0], format!("  n{f} --> n{c}"));
        let json_edges = g
            .edges
            .iter()
            .filter(|e| e.from == fn_id(&g, "f") && e.to == fn_id(&g, "compute"))
            .count();
        assert_eq!(json_edges, 2);
    }

    #[test]
    fn graph_call_seq_follows_source_order() {
        let g = graph(
            "fn main() { a(); b(); c(); }\nfn a() { return; }\nfn b() { return; }\nfn c() { return; }\n",
        );
        let main = fn_id(&g, "main");
        let mut outgoing: Vec<_> = g.edges.iter().filter(|e| e.from == main).collect();
        outgoing.sort_by_key(|e| e.seq);
        assert_eq!(
            outgoing.iter().map(|e| e.seq).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert_eq!(outgoing[0].to, fn_id(&g, "a"));
        assert_eq!(outgoing[1].to, fn_id(&g, "b"));
        assert_eq!(outgoing[2].to, fn_id(&g, "c"));
    }

    #[test]
    fn graph_call_in_args_is_edge_from_caller() {
        let g = graph(
            "fn outer() { wrap(inner()); }\nfn wrap(_x: i32) { return; }\nfn inner() -> i32 { return 1; }\n",
        );
        let outer = fn_id(&g, "outer");
        let wrap = fn_id(&g, "wrap");
        let inner = fn_id(&g, "inner");
        let mut outgoing: Vec<_> = g.edges.iter().filter(|e| e.from == outer).collect();
        outgoing.sort_by_key(|e| e.seq);
        assert_eq!(outgoing.len(), 2);
        assert_eq!(outgoing[0].to, wrap);
        assert_eq!(outgoing[1].to, inner);
        assert!(g.edges.iter().all(|e| e.from == outer));
        match fn_named(&g, "inner") {
            GraphNode::Function { entry, .. } => assert!(!*entry),
            GraphNode::Leaf { .. } => panic!("inner is a function"),
        }
    }

    #[test]
    fn graph_html_embeds_json() {
        let src = include_str!("../tests/fixtures/graph/process_handle/input.rs");
        let files = [("input.rs".into(), src.to_string())];
        let g = graph_files(&files);
        let html = print_graph_html(&g, &files);
        assert!(html.starts_with("<!DOCTYPE html>\n"));
        assert!(!html.contains("<script src"));
        assert!(html.contains("id=\"seer-sources\""));
        assert!(html.contains("fn handle(item: &Item)"));
        let embedded = html_seer_graph_json(&html);
        let json = print_graph_json(&g).replace('<', "\\u003c");
        assert_eq!(embedded, json);
        let handle = fn_named(&g, "handle");
        match handle {
            GraphNode::Function { entry, .. } => assert!(!*entry),
            GraphNode::Leaf { .. } => panic!("handle is a function"),
        }
        let process = fn_named(&g, "process");
        match process {
            GraphNode::Function { entry, .. } => assert!(*entry),
            GraphNode::Leaf { .. } => panic!("process is a function"),
        }
    }
}
