//! Exports what echo's `#[tree]` macro parses, for every tree function in a project, as JSON:
//! for analysis and automated porting.
//!
//! ```text
//! cargo run -p echo_export -- <dir or file>... [--out <dir>]
//! ```
//!
//! Writes `<out>/tree.json`, the structure the macro knows (nodes, properties, signals, calls,
//! branches, loops, keys, state, memos), and `<out>/code.json`, the Rust code it doesn't look
//! into (expressions, statements, patterns), which `tree.json` refers to by key. See README.md.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use echo_export::schema::*;
use proc_macro2::{TokenStream, TokenTree};
use quote::ToTokens;
use syn::visit::Visit;

#[allow(dead_code, clippy::all)]
#[path = "../../echo_macro/src/tree.rs"]
mod tree;

use tree::{Block, BlockElem, CallParams};

fn main() {
    let mut inputs = Vec::new();
    let mut out = PathBuf::from("echo_export");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--out" {
            out = args.next().expect("--out needs a directory").into();
        } else {
            inputs.push(PathBuf::from(arg));
        }
    }
    if inputs.is_empty() {
        eprintln!("usage: echo_export <dir or file>... [--out <dir>]");
        std::process::exit(2);
    }

    let mut files = Vec::new();
    for input in &inputs {
        collect_rs(input, &mut files);
    }
    files.sort();

    let mut ex = Exporter::default();
    for file in &files {
        let text = match fs::read_to_string(file) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("skipping {}: {e}", file.display());
                continue;
            }
        };
        let source = Source::new(file.display().to_string(), text);
        match syn::parse_file(&source.text) {
            Ok(parsed) => {
                let mut finder = Finder {
                    ex: &mut ex,
                    src: &source,
                    impl_ty: None,
                };
                finder.visit_file(&parsed);
            }
            Err(e) => eprintln!("skipping {}: {e}", file.display()),
        }
    }

    fs::create_dir_all(&out).expect("can't create the output directory");
    let (functions, pieces) = (ex.functions.len(), ex.code.len());
    let tree = Tree {
        version: VERSION,
        functions: ex.functions,
    };
    let code = CodeStore {
        version: VERSION,
        code: ex.code,
    };
    write(&out.join("tree.json"), &tree);
    write(&out.join("code.json"), &code);
    eprintln!(
        "{} tree functions in {} files; {} pieces of code -> {}",
        functions,
        files.len(),
        pieces,
        out.display()
    );
}

fn write(path: &Path, value: &impl serde::Serialize) {
    let text = serde_json::to_string_pretty(value).unwrap();
    fs::write(path, text + "\n").unwrap_or_else(|e| panic!("can't write {}: {e}", path.display()));
}

fn collect_rs(path: &Path, files: &mut Vec<PathBuf>) {
    if path.is_file() {
        if path.extension().is_some_and(|e| e == "rs") {
            files.push(path.to_path_buf());
        }
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        let name = entry.file_name();
        if p.is_dir() && (name == "target" || name.to_string_lossy().starts_with('.')) {
            continue;
        }
        collect_rs(&p, files);
    }
}

/// A source file, for slicing the original text of code out of it.
struct Source {
    path: String,
    text: String,
    line_starts: Vec<usize>,
}

impl Source {
    fn new(path: String, text: String) -> Self {
        let line_starts = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        Source {
            path,
            text,
            line_starts,
        }
    }

    /// The byte offset of a line (1-based) and column (0-based, in chars).
    fn offset(&self, lc: proc_macro2::LineColumn) -> usize {
        let start = self.line_starts[lc.line - 1];
        self.text[start..]
            .char_indices()
            .nth(lc.column)
            .map_or(self.text.len(), |(i, _)| start + i)
    }
}

/// The tokens' first and last positions.
fn extent(tokens: &TokenStream) -> Option<(proc_macro2::LineColumn, proc_macro2::LineColumn)> {
    let mut iter = tokens.clone().into_iter();
    let first = iter.next()?;
    let last = iter.last().unwrap_or_else(|| first.clone());
    Some((first.span().start(), last.span().end()))
}

fn line_of(tokens: impl ToTokens) -> usize {
    extent(&tokens.to_token_stream()).map_or(0, |(s, _)| s.line)
}

fn text_of(tokens: impl ToTokens) -> String {
    tokens.to_token_stream().to_string()
}

/// `ON(name)`s in the tokens, anywhere (conditions, statements, property values).
fn signals(tokens: &TokenStream, out: &mut Vec<String>) {
    let tts: Vec<TokenTree> = tokens.clone().into_iter().collect();
    for (i, tt) in tts.iter().enumerate() {
        match tt {
            TokenTree::Ident(id) if id == "ON" => {
                if let Some(TokenTree::Group(g)) = tts.get(i + 1)
                    && g.delimiter() == proc_macro2::Delimiter::Parenthesis
                {
                    let inner: Vec<TokenTree> = g.stream().into_iter().collect();
                    if let [TokenTree::Ident(name)] = inner.as_slice() {
                        out.push(name.to_string());
                    }
                }
            }
            TokenTree::Group(g) => signals(&g.stream(), out),
            _ => {}
        }
    }
}

#[derive(Default)]
struct Exporter {
    functions: Vec<Function>,
    code: BTreeMap<CodeKey, Code>,
    /// The body parameter's name in the function or tree closure being exported.
    body_names: Vec<String>,
}

impl Exporter {
    fn code(&mut self, src: &Source, tokens: impl ToTokens) -> CodeRef {
        let tokens = tokens.to_token_stream();
        let key = CodeKey(self.code.len() as u32 + 1);
        let mut found = Vec::new();
        signals(&tokens, &mut found);
        let (line, column, text) = match extent(&tokens) {
            Some((start, end)) => {
                let (a, b) = (src.offset(start), src.offset(end));
                (
                    start.line,
                    start.column + 1,
                    src.text[a..b.max(a)].to_string(),
                )
            }
            None => (0, 0, String::new()),
        };
        self.code.insert(
            key,
            Code {
                file: src.path.clone(),
                line,
                column,
                text,
                signals: found.clone(),
            },
        );
        CodeRef {
            key,
            signals: found,
        }
    }

    fn block(&mut self, src: &Source, block: Block) -> Vec<Element> {
        block
            .elems
            .into_iter()
            .map(|elem| match elem {
                Ok(elem) => self.elem(src, elem),
                Err(e) => Element::Error {
                    line: Some(e.span().start().line),
                    message: e.to_string(),
                },
            })
            .collect()
    }

    fn parse_body(&mut self, src: &Source, tokens: TokenStream) -> Vec<Element> {
        match syn::parse2::<Block>(tokens) {
            Ok(block) => self.block(src, block),
            Err(e) => vec![Element::Error {
                line: Some(e.span().start().line),
                message: e.to_string(),
            }],
        }
    }

    fn elem(&mut self, src: &Source, elem: BlockElem) -> Element {
        match elem {
            BlockElem::Node { ty, body } => Element::Node {
                line: line_of(&ty),
                node: text_of(&ty),
                body: self.block(src, body),
            },
            BlockElem::LetNode { pat, ty, body } => Element::LetNode {
                line: line_of(&pat),
                pat: self.code(src, &pat),
                node: text_of(&ty),
                body: self.block(src, body),
            },
            BlockElem::Init(list) => {
                let (line, props) = self.props(src, list);
                Element::Init { line, props }
            }
            BlockElem::Update(list) => {
                let (line, props) = self.props(src, list);
                Element::Update { line, props }
            }
            BlockElem::Scope(body) => Element::Block {
                body: self.block(src, body),
            },
            BlockElem::Stmt { tokens, .. } => {
                let line = line_of(&tokens);
                let body_args = self.body_call(&tokens);
                let code = self.code(src, &tokens);
                Element::Code {
                    line,
                    code: code.key,
                    signals: code.signals,
                    // `body(args)`: where the caller's body goes
                    body_call: body_args.is_some(),
                    args: body_args.map(|args| args.iter().map(|a| self.code(src, a)).collect()),
                }
            }
            BlockElem::Call { call, params, body } => {
                Element::Call(self.call(src, None, call, params, body))
            }
            BlockElem::LetCall {
                pat,
                call,
                params,
                body,
            } => Element::LetCall(self.call(src, Some(pat), call, params, body)),
            BlockElem::ViewClosure {
                pat,
                usable,
                closure,
                ..
            } => {
                let line = line_of(&pat);
                let name = self.code(src, &pat);
                let inputs: Vec<syn::Pat> = closure.inputs.iter().cloned().collect();
                let body_param = inputs.last().and_then(|p| match p {
                    syn::Pat::Type(t) if tree::is_body_type(&t.ty) => Some(t.clone()),
                    _ => None,
                });
                let params = inputs
                    .iter()
                    .take(inputs.len() - body_param.is_some() as usize)
                    .map(|p| self.code(src, p))
                    .collect();
                let body_name = body_param
                    .as_ref()
                    .map_or("__body".into(), |t| text_of(&t.pat));
                let body_param = body_param.map(|t| self.body_param(src, &t.pat, &t.ty));
                self.body_names.push(body_name);
                let body = self.parse_body(src, tree::body_tokens(&closure.body));
                self.body_names.pop();
                Element::TreeClosure {
                    line,
                    name,
                    r#in: usable.map(text_of),
                    params,
                    body_param,
                    body,
                }
            }
            BlockElem::If(mut elem) => {
                let line = line_of(&elem.cond);
                let mut branches = Vec::new();
                let mut r#else = None;
                loop {
                    let tree::IfElem {
                        cond,
                        body,
                        else_expr,
                    } = elem;
                    branches.push(Branch {
                        cond: self.code(src, &cond),
                        body: self.block(src, body),
                    });
                    match else_expr {
                        Some(either::Either::Left(next)) => elem = *next,
                        Some(either::Either::Right(body)) => {
                            r#else = Some(self.block(src, body));
                            break;
                        }
                        None => break,
                    }
                }
                Element::If {
                    line,
                    branches,
                    r#else,
                }
            }
            BlockElem::For {
                pat,
                iter,
                key,
                body,
                ..
            } => Element::For {
                line: line_of(&pat),
                pat: self.code(src, &pat),
                iter: self.code(src, &iter),
                key: key.map(|k| self.code(src, &k)),
                body: self.block(src, body),
            },
            BlockElem::Match { value, arms } => Element::Match {
                line: line_of(&value),
                value: self.code(src, &value),
                arms: arms
                    .into_iter()
                    .map(|arm| Arm {
                        pat: self.code(src, &arm.pat),
                        guard: arm.guard.map(|g| self.code(src, &g)),
                        body: self.block(src, arm.body),
                    })
                    .collect(),
            },
            BlockElem::LetState {
                mutability,
                name,
                ty,
                init,
            } => Element::State {
                line: line_of(&name),
                name: name.to_string(),
                r#mut: mutability.is_some(),
                r#type: ty.map(|t| self.code(src, &t)),
                init: self.code(src, &init),
            },
            BlockElem::Memo { value, body } => Element::Memo {
                line: line_of(&value),
                value: self.code(src, &value),
                body: self.block(src, body),
            },
        }
    }

    /// The arguments, if the statement is a call of the current body parameter.
    fn body_call(&self, tokens: &TokenStream) -> Option<Vec<syn::Expr>> {
        let name = self.body_names.last()?;
        let mut iter = tokens.clone().into_iter();
        match (iter.next(), iter.next()) {
            (Some(TokenTree::Ident(id)), Some(TokenTree::Group(g)))
                if id == name && g.delimiter() == proc_macro2::Delimiter::Parenthesis =>
            {
                let args = syn::parse::Parser::parse2(
                    syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated,
                    g.stream(),
                )
                .ok()?;
                Some(args.into_iter().collect())
            }
            _ => None,
        }
    }

    fn props(
        &mut self,
        src: &Source,
        list: syn::punctuated::Punctuated<tree::Assignment, syn::Token![,]>,
    ) -> (usize, Vec<Prop>) {
        let line = list.first().map_or(0, |a| line_of(&a.name));
        let props = list
            .into_iter()
            .map(|a| Prop {
                name: a.name.to_string().trim_start_matches("r#").to_string(),
                args: a
                    .args
                    .map(|args| args.iter().map(|arg| self.code(src, arg)).collect()),
                value: self.code(src, &a.value),
            })
            .collect();
        (line, props)
    }

    fn call(
        &mut self,
        src: &Source,
        pat: Option<syn::Pat>,
        call: syn::Expr,
        params: CallParams,
        body: Block,
    ) -> Call {
        let (callee, args): (String, Vec<&syn::Expr>) = match &call {
            syn::Expr::Call(c) => (text_of(&c.func), c.args.iter().collect()),
            syn::Expr::MethodCall(c) => (
                format!("{}.{}", text_of(&c.receiver), c.method),
                c.args.iter().collect(),
            ),
            other => (text_of(other), Vec::new()),
        };
        Call {
            line: line_of(&call),
            pat: pat.map(|p| self.code(src, &p)),
            callee,
            args: args.iter().map(|a| self.code(src, a)).collect(),
            body_params: match params {
                CallParams::None => None,
                CallParams::Closure { capture, pats } => Some(BodyParams {
                    r#move: capture.is_some(),
                    pats: pats.iter().map(|p| self.code(src, p)).collect(),
                }),
            },
            body: self.block(src, body),
        }
    }

    /// `body: Body<N, A>`: its name, the node type it's placed in, and its arguments' type.
    fn body_param(&mut self, src: &Source, pat: &syn::Pat, ty: &syn::Type) -> BodyParam {
        let mut node = None;
        let mut args = None;
        if let syn::Type::Path(p) = ty
            && let Some(seg) = p.path.segments.last()
            && let syn::PathArguments::AngleBracketed(a) = &seg.arguments
        {
            let types: Vec<&syn::Type> = a
                .args
                .iter()
                .filter_map(|g| match g {
                    syn::GenericArgument::Type(t) => Some(t),
                    _ => None,
                })
                .collect();
            node = types.first().map(text_of);
            args = types.get(1).map(|t| self.code(src, t));
        }
        BodyParam {
            name: text_of(pat),
            node,
            args,
        }
    }

    fn function(
        &mut self,
        src: &Source,
        name: String,
        attr: &syn::Attribute,
        sig: &syn::Signature,
        block: &syn::Block,
    ) {
        let usable = match &attr.meta {
            syn::Meta::List(list) => syn::parse2::<tree::TreeAttr>(list.tokens.clone())
                .ok()
                .and_then(|a| a.usable)
                .map(text_of),
            _ => None,
        };
        let body_param = match sig.inputs.last() {
            Some(syn::FnArg::Typed(t)) if tree::is_body_type(&t.ty) => Some(t.clone()),
            _ => None,
        };
        let params = sig
            .inputs
            .iter()
            .take(sig.inputs.len() - body_param.is_some() as usize)
            .map(|p| self.code(src, p))
            .collect();
        let body_name = body_param
            .as_ref()
            .map_or("__body".into(), |t| text_of(&t.pat));
        let body_param = body_param.map(|t| self.body_param(src, &t.pat, &t.ty));
        let returns = match &sig.output {
            syn::ReturnType::Default => None,
            syn::ReturnType::Type(_, ty) => Some(self.code(src, ty)),
        };
        let tokens: TokenStream = block.stmts.iter().map(|s| s.to_token_stream()).collect();
        self.body_names.push(body_name);
        let body = self.parse_body(src, tokens);
        self.body_names.pop();
        self.functions.push(Function {
            file: src.path.clone(),
            line: sig.ident.span().start().line,
            name,
            r#in: usable,
            params,
            body_param,
            returns,
            body,
        });
    }
}

/// Finds `#[tree]` functions and methods (also nested in other functions).
struct Finder<'a> {
    ex: &'a mut Exporter,
    src: &'a Source,
    impl_ty: Option<String>,
}

fn tree_attr(attrs: &[syn::Attribute]) -> Option<&syn::Attribute> {
    attrs
        .iter()
        .find(|a| a.path().segments.last().is_some_and(|s| s.ident == "tree"))
}

impl<'ast> Visit<'ast> for Finder<'_> {
    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        if let Some(attr) = tree_attr(&f.attrs) {
            self.ex
                .function(self.src, f.sig.ident.to_string(), attr, &f.sig, &f.block);
        }
        syn::visit::visit_item_fn(self, f);
    }

    fn visit_item_impl(&mut self, i: &'ast syn::ItemImpl) {
        let outer = self.impl_ty.replace(text_of(&i.self_ty));
        syn::visit::visit_item_impl(self, i);
        self.impl_ty = outer;
    }

    fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
        if let Some(attr) = tree_attr(&f.attrs) {
            let name = match &self.impl_ty {
                Some(ty) => format!("{ty}::{}", f.sig.ident),
                None => f.sig.ident.to_string(),
            };
            self.ex.function(self.src, name, attr, &f.sig, &f.block);
        }
        syn::visit::visit_impl_item_fn(self, f);
    }
}
