use std::collections::{HashMap, HashSet};

use crate::extract::index_defs;
use crate::ir::{CallKind, CallSite, FnDef, FnId, FnKind, Outline, OutlineNode, RawNode};
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
    pub fn function(&self, id: &FnId) -> Option<&FnDef> {
        self.by_id.get(id).copied().map(|i| &self.defs[i])
    }

    fn module_for_file(&self, file: &str) -> &[String] {
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
    if is_standard_library_crate(&path[0]) {
        return None;
    }
    let name = path.last()?;
    let prefix = &path[..path.len() - 1];
    let mod_path = module_path_from_prefix(prefix, file, index)?;
    if !index.modules.contains(&mod_path) {
        return None;
    }
    unique_function_matching(index, file, |d| d.module == mod_path && d.name == *name)
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
            let Some(mod_path) = module_path_from_prefix(prefix, file, index) else {
                continue;
            };
            if !index.modules.contains(&mod_path) {
                continue;
            }
            for d in free_functions(index).filter(|d| d.module == mod_path && d.name == name) {
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

    let cur_mod = index.module_for_file(file);
    unique_function_matching(index, file, |d| {
        d.module == cur_mod && d.id.file != file && d.name == name
    })
}

fn resolve_imported(bound: &[String], file: &str, index: &ResolveIndex) -> Option<FnId> {
    if bound.is_empty() {
        return None;
    }
    if bound.len() == 1 {
        if is_standard_library_crate(&bound[0]) {
            return None;
        }
        // Single-segment `use foo` is not a function path; treat as 0 defs.
        return None;
    }
    resolve_qualified(bound, file, index)
}

/// Leading `super` is disjoint from `crate` / `self` / else — no fallthrough.
fn module_path_from_prefix(
    prefix: &[String],
    file: &str,
    index: &ResolveIndex,
) -> Option<Vec<String>> {
    if prefix.is_empty() {
        return Some(Vec::new());
    }
    match prefix[0].as_str() {
        "super" => {
            let mut acc = index.module_for_file(file).to_vec();
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
            let mut acc = index.module_for_file(file).to_vec();
            acc.extend(prefix[1..].iter().cloned());
            Some(acc)
        }
        _ => Some(prefix.to_vec()),
    }
}

fn is_standard_library_crate(seg: &str) -> bool {
    LANG_EXTERN.contains(&seg)
}

fn free_functions(index: &ResolveIndex) -> impl Iterator<Item = &FnDef> {
    index
        .defs
        .iter()
        .filter(|d| d.kind == FnKind::Free && d.has_body)
}

fn unique_free_function(index: &ResolveIndex, pred: impl Fn(&FnDef) -> bool) -> Option<FnId> {
    let hits: Vec<&FnDef> = free_functions(index).filter(|d| pred(d)).collect();
    match hits.as_slice() {
        [one] => Some(one.id.clone()),
        _ => None,
    }
}

/// FnIds that some call resolves to as an expand target.
pub(crate) fn called_functions(index: &ResolveIndex) -> HashSet<FnId> {
    let mut called = HashSet::new();
    for def in &index.defs {
        collect_resolved_callees(&def.body, index, &mut called);
    }
    called
}

fn collect_resolved_callees(nodes: &[RawNode], index: &ResolveIndex, called: &mut HashSet<FnId>) {
    for node in nodes {
        match node {
            RawNode::Control { children, .. } | RawNode::NestedFn { children, .. } => {
                collect_resolved_callees(children, index, called);
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
pub(crate) fn select_entries(index: &ResolveIndex, called: &HashSet<FnId>) -> Vec<FnId> {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Expand {
    /// Re-expand a callee at every call site (tree).
    Every,
    /// Expand each callee once; later sites are leaves (flow/diff).
    Once,
}

pub fn outline(index: &ResolveIndex, mode: Expand) -> Outline {
    let called = called_functions(index);
    let mut entries = select_entries(index, &called);
    if mode == Expand::Once {
        entries.sort_by(|a, b| {
            let da = index.function(a).expect("entry is indexed");
            let db = index.function(b).expect("entry is indexed");
            da.id
                .file
                .cmp(&db.id.file)
                .then(da.name.cmp(&db.name))
                .then(da.id.start_byte.cmp(&db.id.start_byte))
        });
    }
    let mut seen = HashSet::new();
    let mut roots = Vec::new();
    for id in entries {
        if mode == Expand::Once && seen.contains(&id) {
            continue;
        }
        let def = index.function(&id).expect("entry is indexed");
        let mut stack = Vec::new();
        let children = match mode {
            Expand::Every => expand_function(def, &mut stack, None, index),
            Expand::Once => expand_function(def, &mut stack, Some(&mut seen), index),
        };
        roots.push(OutlineNode {
            text: format!("fn {}", def.name),
            loc: Some(def.source_location()),
            children,
        });
    }
    Outline { roots }
}

fn expand_function(
    def: &FnDef,
    stack: &mut Vec<FnId>,
    mut seen: Option<&mut HashSet<FnId>>,
    index: &ResolveIndex,
) -> Vec<OutlineNode> {
    if let Some(seen) = seen.as_deref_mut() {
        seen.insert(def.id.clone());
    }
    stack.push(def.id.clone());
    let nodes = expand_body_nodes(&def.body, stack, seen, index);
    stack.pop();
    nodes
}

fn expand_body_nodes(
    raws: &[RawNode],
    stack: &mut Vec<FnId>,
    mut seen: Option<&mut HashSet<FnId>>,
    index: &ResolveIndex,
) -> Vec<OutlineNode> {
    let mut out = Vec::new();
    for raw in raws {
        out.extend(expand_one_node(raw, stack, seen.as_deref_mut(), index));
    }
    out
}

fn expand_one_node(
    raw: &RawNode,
    stack: &mut Vec<FnId>,
    seen: Option<&mut HashSet<FnId>>,
    index: &ResolveIndex,
) -> Vec<OutlineNode> {
    match raw {
        RawNode::Control { text, children } => vec![OutlineNode {
            text: text.clone(),
            loc: None,
            children: expand_body_nodes(children, stack, seen, index),
        }],
        RawNode::NestedFn { name, children } => vec![OutlineNode {
            text: format!("fn {name}"),
            loc: None,
            children: expand_body_nodes(children, stack, seen, index),
        }],
        RawNode::Call { site } => expand_call_site(site, stack, seen, index),
    }
}

fn expand_call_site(
    site: &CallSite,
    stack: &mut Vec<FnId>,
    seen: Option<&mut HashSet<FnId>>,
    index: &ResolveIndex,
) -> Vec<OutlineNode> {
    let target = resolve(site, index);
    let empty = UseMap::default();
    let uses = index.uses.get(&site.file).unwrap_or(&empty);
    if should_omit_resolved(site, uses, target.is_some()) {
        return Vec::new();
    }
    let def = target.as_ref().and_then(|id| index.function(id));
    let loc = def.map(FnDef::source_location);
    let text = site.display.clone();
    let Some(id) = target else {
        return vec![OutlineNode {
            text,
            loc: None,
            children: Vec::new(),
        }];
    };
    if stack.contains(&id) {
        return vec![OutlineNode {
            text: format!("{text} [recursive]"),
            loc,
            children: Vec::new(),
        }];
    }
    if seen.as_ref().is_some_and(|s| s.contains(&id)) {
        return vec![OutlineNode {
            text,
            loc,
            children: Vec::new(),
        }];
    }
    let def = def.expect("resolved id is indexed");
    vec![OutlineNode {
        text,
        loc,
        children: expand_function(def, stack, seen, index),
    }]
}

fn unique_function_matching(
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
        _ => unique_free_function(index, pred),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn free_call_site(file: &str, path: &[&str]) -> CallSite {
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
        let id = resolve(&free_call_site("input.rs", &["handle"]), &index).expect("unique handle");
        assert_eq!(index.function(&id).unwrap().name, "handle");
        assert_eq!(id.file, "input.rs");
    }

    #[test]
    fn resolve_same_file_ambiguous() {
        let index = index_files(&[(
            "input.rs".into(),
            "fn process() { handle(); }\nfn handle() { return; }\nfn handle() { return; }\n".into(),
        )]);
        assert_eq!(
            resolve(&free_call_site("input.rs", &["handle"]), &index),
            None
        );
    }

    #[test]
    fn resolve_std_external() {
        let index = index_files(&[(
            "input.rs".into(),
            "fn f() { std::fs::write(path, data); }\n".into(),
        )]);
        assert_eq!(
            resolve(&free_call_site("input.rs", &["std", "fs", "write"]), &index),
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
