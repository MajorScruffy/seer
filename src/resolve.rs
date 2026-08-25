use std::collections::{HashMap, HashSet};

use crate::extract::index_defs;
use crate::ir::{self, CallKind, CallSite, FnDef, FnId, FnKind, OutlineNode, RawNode};
use crate::lang::{language_for_path, Language};
use crate::omit::{should_omit_resolved, UseMap};
use crate::parse::parse_path;

const LANG_EXTERN: &[&str] = &["std", "core", "alloc", "proc_macro", "test"];

/// Indexed analyzed set: defs, modules, and per-file use maps.
#[derive(Debug)]
pub struct ResolveIndex {
    pub defs: Vec<FnDef>,
    by_id: HashMap<FnId, usize>,
    pub modules: HashSet<Vec<String>>,
    pub uses: HashMap<String, UseMap>,
    file_module: HashMap<String, Vec<String>>,
}

impl ResolveIndex {
    pub fn def(&self, id: &FnId) -> Option<&FnDef> {
        self.by_id.get(id).copied().map(|i| &self.defs[i])
    }

    fn module_of(&self, file: &str) -> &[String] {
        self.file_module.get(file).map(Vec::as_slice).unwrap_or(&[])
    }
}

/// `rel` is per-path: drop a leading `src/` on that path only.
fn rust_module_path(file: &str) -> Vec<String> {
    if file == "<stdin>" {
        return Vec::new();
    }
    let rel = file.strip_prefix("src/").unwrap_or(file);
    let mut parts: Vec<&str> = rel.split('/').filter(|s| !s.is_empty()).collect();
    let Some(file_name) = parts.pop() else {
        return Vec::new();
    };
    if file_name == "lib.rs" || file_name == "main.rs" || file_name == "mod.rs" {
        return parts.into_iter().map(str::to_string).collect();
    }
    let stem = file_name.strip_suffix(".rs").unwrap_or(file_name);
    parts.push(stem);
    parts.into_iter().map(str::to_string).collect()
}

pub fn index_files(files: &[(String, String)]) -> ResolveIndex {
    let mut modules = HashSet::new();
    let mut uses_map = HashMap::new();
    let mut file_module = HashMap::new();
    let mut defs = Vec::new();

    for (path, src) in files {
        let tree = parse_path(path, src);
        let uses = UseMap::for_file(tree.root_node(), src, path);
        let module = match language_for_path(path) {
            Some(Language::Java) => {
                crate::lang::java::module_path_from_tree(tree.root_node(), path, src)
            }
            Some(Language::TypeScript) => crate::lang::typescript::module_path(path),
            _ => rust_module_path(path),
        };
        modules.insert(module.clone());
        uses_map.insert(path.clone(), uses.clone());
        file_module.insert(path.clone(), module.clone());
        defs.extend(index_defs(tree.root_node(), src, path, &uses, &module));
    }

    let mut by_id = HashMap::with_capacity(defs.len());
    for (i, def) in defs.iter().enumerate() {
        by_id.insert(def.id.clone(), i);
    }

    ResolveIndex {
        defs,
        by_id,
        modules,
        uses: uses_map,
        file_module,
    }
}

pub fn resolve(site: &CallSite, index: &ResolveIndex) -> Option<FnId> {
    // Macros never resolve to a function_item; omit still uses the path.
    if site.is_macro {
        return None;
    }
    match &site.kind {
        CallKind::Method { name } => resolve_method(name, index),
        CallKind::Free { path } => resolve_free(path, &site.file, index),
    }
}

fn resolve_method(name: &str, index: &ResolveIndex) -> Option<FnId> {
    let hits: Vec<&FnDef> = index.defs.iter().filter(|d| d.name == name).collect();
    match hits.as_slice() {
        [one] if one.has_body => Some(one.id.clone()),
        _ => None,
    }
}

fn resolve_free(path: &[String], file: &str, index: &ResolveIndex) -> Option<FnId> {
    if path.is_empty() {
        return None;
    }
    if path.len() >= 2 {
        return resolve_qualified(path, file, index);
    }
    resolve_unqualified(&path[0], file, index)
}

fn resolve_qualified(path: &[String], file: &str, index: &ResolveIndex) -> Option<FnId> {
    if is_lang_external(&path[0]) {
        return None;
    }
    let name = path.last()?;
    let prefix = &path[..path.len() - 1];
    let mod_path = map_prefix_to_module(prefix, file, index)?;
    if !index.modules.contains(&mod_path) {
        return None;
    }
    unique_callable(index, file, |d| d.module == mod_path && d.name == *name)
}

fn resolve_unqualified(name: &str, file: &str, index: &ResolveIndex) -> Option<FnId> {
    let same_file: Vec<&FnDef> = index
        .defs
        .iter()
        .filter(|d| d.id.file == file && d.name == name && d.has_body)
        .collect();
    match same_file.as_slice() {
        [one] => return Some(one.id.clone()),
        [] => {}
        _ => return None,
    }

    if let Some(uses) = index.uses.get(file) {
        if let Some(bound) = uses.binding(name) {
            return resolve_imported(bound, file, index);
        }
        let mut ids: Vec<FnId> = Vec::new();
        for prefix in uses.globs() {
            let Some(mod_path) = map_prefix_to_module(prefix, file, index) else {
                continue;
            };
            if !index.modules.contains(&mod_path) {
                continue;
            }
            for d in free_defs(index).filter(|d| d.module == mod_path && d.name == name) {
                if !ids.contains(&d.id) {
                    ids.push(d.id.clone());
                }
            }
        }
        match ids.as_slice() {
            [one] => return Some(one.clone()),
            [] => {}
            _ => return None,
        }
    }

    let cur_mod = index.module_of(file);
    unique_callable(index, file, |d| {
        d.module == cur_mod && d.id.file != file && d.name == name
    })
}

fn resolve_imported(bound: &[String], file: &str, index: &ResolveIndex) -> Option<FnId> {
    if bound.is_empty() {
        return None;
    }
    if bound.len() == 1 {
        if is_lang_external(&bound[0]) {
            return None;
        }
        // Single-segment `use foo` is not a function path; treat as 0 defs.
        return None;
    }
    resolve_qualified(bound, file, index)
}

/// Leading `super` is disjoint from `crate` / `self` / else — no fallthrough.
fn map_prefix_to_module(
    prefix: &[String],
    file: &str,
    index: &ResolveIndex,
) -> Option<Vec<String>> {
    if prefix.is_empty() {
        return Some(Vec::new());
    }
    match prefix[0].as_str() {
        "super" => {
            let mut acc = index.module_of(file).to_vec();
            let mut rest = prefix;
            while rest.first().map(String::as_str) == Some("super") {
                if acc.is_empty() {
                    return None;
                }
                acc.pop();
                rest = &rest[1..];
            }
            acc.extend(rest.iter().cloned());
            Some(acc)
        }
        "crate" => Some(prefix[1..].to_vec()),
        "self" => {
            let mut acc = index.module_of(file).to_vec();
            acc.extend(prefix[1..].iter().cloned());
            Some(acc)
        }
        _ => Some(prefix.to_vec()),
    }
}

fn is_lang_external(seg: &str) -> bool {
    LANG_EXTERN.contains(&seg)
}

fn free_defs(index: &ResolveIndex) -> impl Iterator<Item = &FnDef> {
    index
        .defs
        .iter()
        .filter(|d| d.kind == FnKind::Free && d.has_body)
}

fn unique_free(index: &ResolveIndex, pred: impl Fn(&FnDef) -> bool) -> Option<FnId> {
    let hits: Vec<&FnDef> = free_defs(index).filter(|d| pred(d)).collect();
    match hits.as_slice() {
        [one] => Some(one.id.clone()),
        _ => None,
    }
}

/// FnIds that some non-omitted call resolves to as an expand target.
pub fn called_targets(index: &ResolveIndex) -> HashSet<FnId> {
    let mut called = HashSet::new();
    for def in &index.defs {
        collect_calls(&def.body, index, &mut called);
    }
    called
}

fn collect_calls(nodes: &[RawNode], index: &ResolveIndex, called: &mut HashSet<FnId>) {
    for node in nodes {
        match node {
            RawNode::Control { children, .. } | RawNode::NestedFn { children, .. } => {
                collect_calls(children, index, called);
            }
            RawNode::Call { site } => {
                if let Some(id) = resolve(site, index) {
                    called.insert(id);
                }
            }
        }
    }
}

/// Non-nested body-bearing defs that no call expands to; cycle fallback if none.
pub fn select_entries(index: &ResolveIndex, called: &HashSet<FnId>) -> Vec<FnId> {
    let candidates: Vec<&FnDef> = index
        .defs
        .iter()
        .filter(|d| d.has_body && !d.nested)
        .collect();
    let mut entries: Vec<FnId> = candidates
        .iter()
        .filter(|d| !called.contains(&d.id))
        .map(|d| d.id.clone())
        .collect();
    if entries.is_empty() && !candidates.is_empty() {
        entries = candidates.iter().map(|d| d.id.clone()).collect();
    }
    entries.sort_by(|a, b| a.file.cmp(&b.file).then(a.start_byte.cmp(&b.start_byte)));
    entries
}

pub fn expand_fn(def: &FnDef, stack: &mut Vec<FnId>, index: &ResolveIndex) -> Vec<OutlineNode> {
    stack.push(def.id.clone());
    let nodes = expand_nodes(&def.body, stack, index);
    stack.pop();
    nodes
}

fn expand_nodes(raws: &[RawNode], stack: &mut Vec<FnId>, index: &ResolveIndex) -> Vec<OutlineNode> {
    raws.iter()
        .flat_map(|raw| expand_raw(raw, stack, index))
        .collect()
}

fn expand_raw(raw: &RawNode, stack: &mut Vec<FnId>, index: &ResolveIndex) -> Vec<OutlineNode> {
    match raw {
        RawNode::Control { text, children } => vec![OutlineNode {
            text: text.clone(),
            children: expand_nodes(children, stack, index),
        }],
        RawNode::NestedFn { name, children } => vec![OutlineNode {
            text: format!("fn {name}"),
            children: expand_nodes(children, stack, index),
        }],
        RawNode::Call { site } => expand_call(site, stack, index),
    }
}

fn expand_call(site: &CallSite, stack: &mut Vec<FnId>, index: &ResolveIndex) -> Vec<OutlineNode> {
    let target = resolve(site, index);
    let empty = UseMap::default();
    let uses = index.uses.get(&site.file).unwrap_or(&empty);
    if should_omit_resolved(site, uses, target.is_some()) {
        return Vec::new();
    }
    let text = site.display.clone();
    let Some(id) = target else {
        return vec![OutlineNode {
            text,
            children: Vec::new(),
        }];
    };
    if stack.contains(&id) {
        return vec![OutlineNode {
            text: format!("{text} [recursive]"),
            children: Vec::new(),
        }];
    }
    let def = index.def(&id).expect("resolved id is indexed");
    vec![OutlineNode {
        text,
        children: expand_fn(def, stack, index),
    }]
}

/// Entry-rooted flow: each callee expands once. Roots sorted by file then name
/// so moving a definition in a file does not change the outline.
pub fn flow_outline(index: &ResolveIndex) -> String {
    let called = called_targets(index);
    let mut entries = select_entries(index, &called);
    entries.sort_by(|a, b| {
        let da = index.def(a).expect("entry is indexed");
        let db = index.def(b).expect("entry is indexed");
        da.id
            .file
            .cmp(&db.id.file)
            .then(da.name.cmp(&db.name))
            .then(da.id.start_byte.cmp(&db.id.start_byte))
    });
    let mut seen = HashSet::new();
    let mut roots = Vec::new();
    for id in entries {
        if seen.contains(&id) {
            continue;
        }
        let def = index.def(&id).expect("entry is indexed");
        let mut stack = Vec::new();
        let children = expand_once(def, &mut stack, &mut seen, index);
        roots.push(OutlineNode {
            text: format!("fn {}", def.name),
            children,
        });
    }
    ir::print(&ir::Outline { roots })
}

fn expand_once(
    def: &FnDef,
    stack: &mut Vec<FnId>,
    seen: &mut HashSet<FnId>,
    index: &ResolveIndex,
) -> Vec<OutlineNode> {
    seen.insert(def.id.clone());
    stack.push(def.id.clone());
    let nodes = expand_once_nodes(&def.body, stack, seen, index);
    stack.pop();
    nodes
}

fn expand_once_nodes(
    raws: &[RawNode],
    stack: &mut Vec<FnId>,
    seen: &mut HashSet<FnId>,
    index: &ResolveIndex,
) -> Vec<OutlineNode> {
    raws.iter()
        .flat_map(|raw| expand_once_raw(raw, stack, seen, index))
        .collect()
}

fn expand_once_raw(
    raw: &RawNode,
    stack: &mut Vec<FnId>,
    seen: &mut HashSet<FnId>,
    index: &ResolveIndex,
) -> Vec<OutlineNode> {
    match raw {
        RawNode::Control { text, children } => vec![OutlineNode {
            text: text.clone(),
            children: expand_once_nodes(children, stack, seen, index),
        }],
        RawNode::NestedFn { name, children } => vec![OutlineNode {
            text: format!("fn {name}"),
            children: expand_once_nodes(children, stack, seen, index),
        }],
        RawNode::Call { site } => expand_once_call(site, stack, seen, index),
    }
}

fn expand_once_call(
    site: &CallSite,
    stack: &mut Vec<FnId>,
    seen: &mut HashSet<FnId>,
    index: &ResolveIndex,
) -> Vec<OutlineNode> {
    let target = resolve(site, index);
    let empty = UseMap::default();
    let uses = index.uses.get(&site.file).unwrap_or(&empty);
    if should_omit_resolved(site, uses, target.is_some()) {
        return Vec::new();
    }
    let text = site.display.clone();
    let Some(id) = target else {
        return vec![OutlineNode {
            text,
            children: Vec::new(),
        }];
    };
    if stack.contains(&id) {
        return vec![OutlineNode {
            text: format!("{text} [recursive]"),
            children: Vec::new(),
        }];
    }
    if seen.contains(&id) {
        return vec![OutlineNode {
            text,
            children: Vec::new(),
        }];
    }
    let def = index.def(&id).expect("resolved id is indexed");
    vec![OutlineNode {
        text,
        children: expand_once(def, stack, seen, index),
    }]
}

pub fn flow_diff(left: &ResolveIndex, right: &ResolveIndex) -> (String, String) {
    (flow_outline(left), flow_outline(right))
}

fn unique_callable(
    index: &ResolveIndex,
    file: &str,
    pred: impl Fn(&FnDef) -> bool,
) -> Option<FnId> {
    match language_for_path(file) {
        Some(Language::Java) | Some(Language::TypeScript) => {
            let hits: Vec<&FnDef> = index
                .defs
                .iter()
                .filter(|d| d.has_body && pred(d))
                .collect();
            match hits.as_slice() {
                [one] => Some(one.id.clone()),
                _ => None,
            }
        }
        _ => unique_free(index, pred),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site_free(file: &str, path: &[&str]) -> CallSite {
        CallSite {
            display: path.join("::"),
            kind: CallKind::Free {
                path: path.iter().map(|s| (*s).to_string()).collect(),
            },
            is_macro: false,
            file: file.to_string(),
        }
    }

    #[test]
    fn resolve_same_file_win() {
        let index = index_files(&[(
            "input.rs".into(),
            "fn process() { handle(); }\nfn handle() { return; }\n".into(),
        )]);
        let id = resolve(&site_free("input.rs", &["handle"]), &index).expect("unique handle");
        assert_eq!(index.def(&id).unwrap().name, "handle");
        assert_eq!(id.file, "input.rs");
    }

    #[test]
    fn resolve_same_file_ambiguous() {
        let index = index_files(&[(
            "input.rs".into(),
            "fn process() { handle(); }\nfn handle() { return; }\nfn handle() { return; }\n".into(),
        )]);
        assert_eq!(resolve(&site_free("input.rs", &["handle"]), &index), None);
    }

    #[test]
    fn resolve_std_external() {
        let index = index_files(&[(
            "input.rs".into(),
            "fn f() { std::fs::write(path, data); }\n".into(),
        )]);
        assert_eq!(
            resolve(&site_free("input.rs", &["std", "fs", "write"]), &index),
            None
        );
    }

    #[test]
    fn module_path_src_strip_is_per_path() {
        assert_eq!(rust_module_path("src/lib.rs"), Vec::<String>::new());
        assert_eq!(rust_module_path("src/foo.rs"), vec!["foo".to_string()]);
        assert_eq!(rust_module_path("root.rs"), vec!["root".to_string()]);
        assert_eq!(rust_module_path("lib.rs"), Vec::<String>::new());
        assert_eq!(rust_module_path("<stdin>"), Vec::<String>::new());
        assert_eq!(
            rust_module_path("foo/bar.rs"),
            vec!["foo".to_string(), "bar".to_string()]
        );
    }
}
