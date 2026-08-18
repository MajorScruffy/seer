use std::collections::HashSet;

use crate::ir::{FnDef, FnId, RawNode};
use crate::resolve::{resolve, ResolveIndex};

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
