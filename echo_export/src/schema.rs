//! The format `echo_export` writes, as Rust types: `tree.json` is a [`Tree`], `code.json` a
//! [`CodeStore`]. They (de)serialize with serde; [`Export::load`] reads both.
//!
//! ```no_run
//! use echo_export::schema::{Element, Export, walk};
//!
//! let export = Export::load("export").unwrap();
//! for function in &export.tree.functions {
//!     walk(&function.body, &mut |e| {
//!         if let Element::Node { node, line, .. } = e {
//!             println!("{}:{line}: {node}", function.file);
//!         }
//!     });
//! }
//! ```

use std::{collections::BTreeMap, fmt, fs, io, path::Path, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

/// The format's version, in both files.
pub const VERSION: u32 = 1;

/// Both files of an export.
#[derive(Debug, Clone, PartialEq)]
pub struct Export {
    pub tree: Tree,
    pub code: CodeStore,
}

impl Export {
    /// Reads `tree.json` and `code.json` from `dir`.
    pub fn load(dir: impl AsRef<Path>) -> io::Result<Self> {
        let dir = dir.as_ref();
        let read = |name: &str| -> io::Result<String> { fs::read_to_string(dir.join(name)) };
        let tree = serde_json::from_str(&read("tree.json")?)?;
        let code = serde_json::from_str(&read("code.json")?)?;
        Ok(Export { tree, code })
    }

    /// The code a [`CodeRef`] (or a [`CodeKey`]) refers to.
    pub fn code(&self, key: impl Into<CodeKey>) -> &Code {
        &self.code[key.into()]
    }
}

// ---- tree.json ----------------------------------------------------------------------------------

/// `tree.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tree {
    pub version: u32,
    pub functions: Vec<Function>,
}

/// A `#[tree]` function (or method).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Function {
    pub file: String,
    pub line: usize,
    /// `"name"`, or `"Type::name"` for a method.
    pub name: String,
    /// `#[tree(in Control)]`'s node type, as written.
    pub r#in: Option<String>,
    /// The parameters (each whole, `name: Type`), without the body parameter.
    pub params: Vec<CodeRef>,
    pub body_param: Option<BodyParam>,
    pub returns: Option<CodeRef>,
    pub body: Vec<Element>,
}

/// A `body: Body<N, A>` parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BodyParam {
    pub name: String,
    /// `N`, the node type the body is placed in, as written (`None` if not given).
    pub node: Option<String>,
    /// `A`, the type of what the body is given.
    pub args: Option<CodeRef>,
}

/// An element of a tree function's body.
///
/// What an `Init`/`Update` or an `ON(..)` applies to follows from where it is: the innermost
/// `Node`/`LetNode` around it; in a call's body, the callee's [`BodyParam::node`]; at a
/// function's top level, its [`Function::in`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Element {
    /// `Label..{ .. };`
    Node {
        line: usize,
        /// The type, as written (a path: `Label`, `my::Widget`).
        node: String,
        body: Vec<Element>,
    },
    /// `let a = Label..{ .. };`
    LetNode {
        line: usize,
        pat: CodeRef,
        node: String,
        body: Vec<Element>,
    },
    /// `INIT(..);`
    Init { line: usize, props: Vec<Prop> },
    /// `UPDATE(..);`
    Update { line: usize, props: Vec<Prop> },
    /// `{ .. }`
    Block { body: Vec<Element> },
    /// Any other statement (or item).
    Code {
        line: usize,
        code: CodeKey,
        /// The `ON(..)`s in it.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        signals: Vec<String>,
        /// Whether it's `body(args)`: placing the caller's body.
        #[serde(default, skip_serializing_if = "is_false")]
        body_call: bool,
        /// `body(args)`'s arguments.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        args: Option<Vec<CodeRef>>,
    },
    /// `f(args)..{ .. };`
    Call(Call),
    /// `let v = f(args)..{ .. };`
    LetCall(Call),
    /// `#[tree] let row = |..| { .. };`
    TreeClosure {
        line: usize,
        /// The pattern it's bound to (usually just a name).
        name: CodeRef,
        r#in: Option<String>,
        params: Vec<CodeRef>,
        body_param: Option<BodyParam>,
        body: Vec<Element>,
    },
    /// `if c { .. } else if d { .. } else { .. }`
    If {
        line: usize,
        branches: Vec<Branch>,
        r#else: Option<Vec<Element>>,
    },
    /// `match v { .. }`
    Match {
        line: usize,
        value: CodeRef,
        arms: Vec<Arm>,
    },
    /// `for x in xs { KEY(k); .. }`
    For {
        line: usize,
        pat: CodeRef,
        iter: CodeRef,
        key: Option<CodeRef>,
        body: Vec<Element>,
    },
    /// `let [mut] x[: T] = STATE(init);`
    State {
        line: usize,
        name: String,
        r#mut: bool,
        r#type: Option<CodeRef>,
        init: CodeRef,
    },
    /// `MEMO(v) >> { .. };`
    Memo {
        line: usize,
        value: CodeRef,
        body: Vec<Element>,
    },
    /// Something the macro couldn't parse.
    Error {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        line: Option<usize>,
        message: String,
    },
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// One property of an `INIT`/`UPDATE`: `name = value`, or `name(args) = value`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Prop {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<CodeRef>>,
    pub value: CodeRef,
}

/// A call of a tree function, with a body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Call {
    pub line: usize,
    /// What a `let v = f()..{}` binds (only for [`Element::LetCall`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pat: Option<CodeRef>,
    /// The function, as written (`receiver.method` for a method call).
    pub callee: String,
    pub args: Vec<CodeRef>,
    /// `..|a, b| { .. }`: what the body takes (`None` for `..{ .. }`).
    pub body_params: Option<BodyParams>,
    pub body: Vec<Element>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BodyParams {
    pub r#move: bool,
    pub pats: Vec<CodeRef>,
}

/// A branch of an [`Element::If`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Branch {
    pub cond: CodeRef,
    pub body: Vec<Element>,
}

/// An arm of an [`Element::Match`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Arm {
    pub pat: CodeRef,
    pub guard: Option<CodeRef>,
    pub body: Vec<Element>,
}

impl Element {
    /// The line it starts on (blocks have none, nor do some errors).
    pub fn line(&self) -> Option<usize> {
        match self {
            Element::Node { line, .. }
            | Element::LetNode { line, .. }
            | Element::Init { line, .. }
            | Element::Update { line, .. }
            | Element::Code { line, .. }
            | Element::TreeClosure { line, .. }
            | Element::If { line, .. }
            | Element::Match { line, .. }
            | Element::For { line, .. }
            | Element::State { line, .. }
            | Element::Memo { line, .. } => Some(*line),
            Element::Call(c) | Element::LetCall(c) => Some(c.line),
            Element::Block { .. } => None,
            Element::Error { line, .. } => *line,
        }
    }

    /// The element lists nested in it (bodies, branches, arms, `else`), in order.
    pub fn bodies(&self) -> Vec<&[Element]> {
        match self {
            Element::Node { body, .. }
            | Element::LetNode { body, .. }
            | Element::Block { body }
            | Element::TreeClosure { body, .. }
            | Element::For { body, .. }
            | Element::Memo { body, .. } => vec![body],
            Element::Call(c) | Element::LetCall(c) => vec![&c.body],
            Element::If {
                branches, r#else, ..
            } => branches
                .iter()
                .map(|b| &b.body[..])
                .chain(r#else.as_deref())
                .collect(),
            Element::Match { arms, .. } => arms.iter().map(|a| &a.body[..]).collect(),
            Element::Init { .. }
            | Element::Update { .. }
            | Element::Code { .. }
            | Element::State { .. }
            | Element::Error { .. } => vec![],
        }
    }
}

/// Calls `f` with every element, depth first (each before what's nested in it).
pub fn walk<'a>(elements: &'a [Element], f: &mut impl FnMut(&'a Element)) {
    for e in elements {
        f(e);
        for body in e.bodies() {
            walk(body, f);
        }
    }
}

// ---- code.json ----------------------------------------------------------------------------------

/// `code.json`: the Rust code `tree.json` refers to, by key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeStore {
    pub version: u32,
    pub code: BTreeMap<CodeKey, Code>,
}

impl std::ops::Index<CodeKey> for CodeStore {
    type Output = Code;
    fn index(&self, key: CodeKey) -> &Code {
        self.code
            .get(&key)
            .unwrap_or_else(|| panic!("no code {key} in code.json"))
    }
}

/// A piece of code, as written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Code {
    pub file: String,
    /// 1-based.
    pub line: usize,
    /// 1-based, in characters.
    pub column: usize,
    /// The original source, formatting and comments included.
    pub text: String,
    /// The `ON(..)`s in it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub signals: Vec<String>,
}

/// A key into `code.json`: `"c42"` in the JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CodeKey(pub u32);

impl fmt::Display for CodeKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "c{}", self.0)
    }
}

impl FromStr for CodeKey {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        s.strip_prefix('c')
            .and_then(|n| n.parse().ok())
            .map(CodeKey)
            .ok_or_else(|| format!("not a code key: {s:?}"))
    }
}

impl Serialize for CodeKey {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for CodeKey {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?.parse().map_err(de::Error::custom)
    }
}

/// A reference to code: `"c42"` in the JSON, or `{ "code": "c42", "signals": [..] }` when the
/// code has `ON(..)`s in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "CodeRefRepr", into = "CodeRefRepr")]
pub struct CodeRef {
    pub key: CodeKey,
    /// The `ON(..)`s in it.
    pub signals: Vec<String>,
}

impl From<&CodeRef> for CodeKey {
    fn from(r: &CodeRef) -> CodeKey {
        r.key
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum CodeRefRepr {
    Key(CodeKey),
    WithSignals { code: CodeKey, signals: Vec<String> },
}

impl From<CodeRefRepr> for CodeRef {
    fn from(r: CodeRefRepr) -> Self {
        match r {
            CodeRefRepr::Key(key) => CodeRef {
                key,
                signals: Vec::new(),
            },
            CodeRefRepr::WithSignals { code, signals } => CodeRef { key: code, signals },
        }
    }
}

impl From<CodeRef> for CodeRefRepr {
    fn from(r: CodeRef) -> Self {
        if r.signals.is_empty() {
            CodeRefRepr::Key(r.key)
        } else {
            CodeRefRepr::WithSignals {
                code: r.key,
                signals: r.signals,
            }
        }
    }
}
