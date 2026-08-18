use crate::ir::{CallSite, FnDef, FnId, OutlineNode, RawNode};
use crate::omit::{should_omit_resolved, UseMap};
use crate::resolve::{resolve, ResolveIndex};

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
