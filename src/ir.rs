/// Identity of a `function_item` in the analyzed set.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FnId {
    /// POSIX relative path, or `<stdin>`.
    pub file: String,
    pub start_byte: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FnKind {
    Free,
    Method,
}

/// Indexed `function_item` or `function_signature_item`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FnDef {
    pub id: FnId,
    pub name: String,
    pub kind: FnKind,
    pub module: Vec<String>,
    pub nested: bool,
    pub has_body: bool,
    pub body: Vec<RawNode>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outline {
    pub roots: Vec<OutlineNode>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutlineNode {
    pub text: String,
    pub children: Vec<OutlineNode>,
}

/// Unexpanded extraction node. Resolve keys are not printed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RawNode {
    Control {
        text: String,
        children: Vec<RawNode>,
    },
    NestedFn {
        name: String,
        children: Vec<RawNode>,
    },
    Call {
        site: CallSite,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallSite {
    /// Already collapsed + std-prefix-stripped.
    pub display: String,
    pub kind: CallKind,
    pub is_macro: bool,
    /// Same string as `FnId.file` of the source that contains this call.
    pub file: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallKind {
    /// `foo()`, `foo::bar()`, `crate::foo()`, `todo!()`
    Free { path: Vec<String> },
    /// `recv.method(...)` — name is the last segment
    Method { name: String },
}
