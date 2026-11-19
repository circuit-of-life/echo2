use std::mem;

use either::Either::{self, Left, Right};
use proc_macro2::{Delimiter, Group, Spacing, TokenStream, TokenTree};
use quote::{ToTokens, format_ident, quote, quote_spanned};
use syn::{
    Expr, Ident, LitStr, Pat, Token, braced,
    ext::IdentExt,
    parenthesized,
    parse::{Parse, ParseStream, discouraged::Speculative},
    parse_quote, parse_quote_spanned, parse2,
    punctuated::Punctuated,
    spanned::Spanned,
    token,
    visit_mut::{self, VisitMut},
};

mod kw {
    syn::custom_keyword!(INIT);
    syn::custom_keyword!(UPDATE);
    syn::custom_keyword!(ON);
    syn::custom_keyword!(KEY);
    syn::custom_keyword!(BODY);
    syn::custom_keyword!(ARGS);
    syn::custom_keyword!(STATE);
    syn::custom_keyword!(MEMO);
    syn::custom_keyword!(macro_rules);
}

const EXPECTED_ELEMENT: &str = "expected an element: `Node..{ .. };`, `func(..)..{ .. };`, \
    INIT/UPDATE/BODY(..), `let x = STATE(..);`, `MEMO(..) >> { .. };`, `if`/`for`/`match`, \
    a `{ .. }` block, or a Rust statement";

/// A list of elements. Elements that fail to parse are kept as errors, so the rest of the
/// function still expands (and rust-analyzer keeps working in it).
pub struct Block {
    pub elems: Vec<syn::Result<BlockElem>>,
}

#[allow(clippy::large_enum_variant)] // parse-time only
pub enum BlockElem {
    Node {
        ty: syn::Path,
        body: Block,
    },
    /// `let pat = Node..{ .. };`: a node, whose `Gd` is given to the code after it.
    LetNode {
        pat: Pat,
        ty: syn::Path,
        body: Block,
    },
    Init(Punctuated<Assignment, Token![,]>),
    Update(Punctuated<Assignment, Token![,]>),
    /// `{ .. }`: a block scope, as in Rust (its contents are elements too).
    Scope(Block),
    /// `let ..;` or any other Rust statement. `semi`: whether it has its `;` (only the last
    /// statement of a block can go without, which makes it a function's return value).
    Stmt {
        tokens: TokenStream,
        semi: bool,
    },
    /// `func(..)..{ .. };` or `func(..)..|args| { .. };`
    Call {
        call: Expr,
        params: CallParams,
        body: Block,
    },
    /// `let pat = func(..)..{ .. };`: a call whose return value is kept.
    LetCall {
        pat: Pat,
        call: Expr,
        params: CallParams,
        body: Block,
    },
    /// `#[tree] let name = |..| { .. };`: a tree closure, called like a tree function.
    ViewClosure {
        attrs: Vec<syn::Attribute>,
        pat: Pat,
        usable: Option<syn::Path>,
        closure: syn::ExprClosure,
    },
    If(IfElem),
    /// `for pat in iter { [KEY(key);] .. }`. Without a `KEY`, it's a plain loop, which can't
    /// create anything that has an identity.
    For {
        for_token: Token![for],
        pat: Pat,
        iter: Expr,
        key: Option<Expr>,
        body: Block,
    },
    Match {
        value: Expr,
        arms: Vec<Arm>,
    },
    /// `let [mut] name[: T] = STATE(init);`
    LetState {
        mutability: Option<Token![mut]>,
        name: Ident,
        ty: Option<Box<syn::Type>>,
        init: Expr,
    },
    Memo {
        value: Expr,
        body: Block,
    },
}
/// How a call's body receives the arguments it's given.
pub enum CallParams {
    /// `func(..)..{ .. }`: none (or ignored)
    None,
    /// `func(..)..|a, b| { .. }`
    Closure {
        capture: Option<Token![move]>,
        pats: Vec<Pat>,
    },
}

/// `#[tree]` or `#[tree(in Parent)]`: the node a tree function or closure can be used in.
pub struct TreeAttr {
    pub usable: Option<syn::Path>,
}

impl Parse for TreeAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let usable = if input.peek(Token![in]) {
            input.parse::<Token![in]>()?;
            Some(input.parse()?)
        } else {
            None
        };
        if !input.is_empty() {
            return Err(input.error(
                "expected `#[tree]` or `#[tree(in ParentNode)]`. (The old `#[tree(BodyNode(Args) in Parent)]` \
                 is gone: take the body as the last parameter, `body: Body<BodyNode, Args>`.)",
            ));
        }
        Ok(TreeAttr { usable })
    }
}

pub struct IfElem {
    pub cond: Expr,
    pub body: Block,
    pub else_expr: Option<Either<Box<IfElem>, Block>>,
}
pub struct Arm {
    pub pat: Pat,
    pub guard: Option<Expr>,
    pub body: Block,
}
/// `name = value`, or `name(args..) = value`: `__node.set_name(args.., value)`.
pub struct Assignment {
    pub name: Ident,
    pub args: Option<Punctuated<Expr, Token![,]>>,
    pub value: Expr,
}

/// Parses `T` from the whole of `input` (allowing a trailing comma), rejecting leftovers.
fn parse_all<T: Parse>(input: ParseStream) -> syn::Result<T> {
    let v = input.parse()?;
    // rustfmt adds one when it splits the parentheses over several lines
    if input.peek(Token![,]) {
        input.parse::<Token![,]>()?;
    }
    if !input.is_empty() {
        return Err(input.error("unexpected tokens"));
    }
    Ok(v)
}

fn parse_assignments(input: ParseStream) -> syn::Result<Punctuated<Assignment, Token![,]>> {
    let inner;
    parenthesized!(inner in input);
    let list = Punctuated::parse_terminated(&inner)?;
    end_elem(input)?;
    Ok(list)
}

fn parse_braced_block(input: ParseStream) -> syn::Result<Block> {
    let inner;
    braced!(inner in input);
    inner.parse()
}

/// The `;` ending an element, optional for the last one in a block (like a tail expression).
fn end_elem(input: ParseStream) -> syn::Result<()> {
    if !input.is_empty() {
        input.parse::<Token![;]>()?;
    }
    Ok(())
}

fn eat_semi(input: ParseStream) -> syn::Result<()> {
    if input.peek(Token![;]) {
        input.parse::<Token![;]>()?;
    }
    Ok(())
}

impl Parse for IfElem {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        input.parse::<Token![if]>()?;
        let cond = Expr::parse_without_eager_brace(input)?;
        let body = parse_braced_block(input)?;
        let else_expr = if input.peek(Token![else]) {
            input.parse::<Token![else]>()?;
            Some(if input.peek(Token![if]) {
                Left(Box::new(input.parse()?))
            } else {
                Right(parse_braced_block(input)?)
            })
        } else {
            None
        };

        Ok(Self {
            cond,
            body,
            else_expr,
        })
    }
}

impl Parse for Assignment {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name = input.call(Ident::parse_any)?;
        let args = if input.peek(token::Paren) {
            let inner;
            parenthesized!(inner in input);
            Some(Punctuated::parse_terminated(&inner)?)
        } else {
            None
        };
        input.parse::<Token![=]>()?;
        let value = input.parse()?;
        Ok(Assignment { name, args, value })
    }
}

/// What a statement that isn't a keyword or control flow turned out to be.
enum Split {
    /// `<expr> .. { block }`: a node or a call.
    Range(TokenStream),
    /// `<call> .. |args| body`: a call whose body takes arguments (the input is left at `|`).
    RangeClosure(TokenStream),
    /// A plain Rust statement, and whether it has its `;`.
    Stmt(TokenStream, bool),
}

/// Scans one statement: up to a `..` followed by a `{..}` group, or up to a `;`, or (for
/// statements like `while`, `loop`, `unsafe`, labeled blocks, `macro! { .. }`) up to its block.
fn split_statement(input: ParseStream) -> syn::Result<Split> {
    input.step(|cursor| {
        let mut toks: Vec<TokenTree> = vec![];
        let mut rest = *cursor;
        let mut prev_joint_dot = false;
        let ends_at_block = match cursor.token_tree() {
            Some((TokenTree::Ident(i), _)) => i == "while" || i == "loop" || i == "unsafe",
            Some((TokenTree::Punct(p), _)) => p.as_char() == '\'',
            _ => false,
        };
        while let Some((tt, next)) = rest.token_tree() {
            if let TokenTree::Group(g) = &tt
                && g.delimiter() == Delimiter::Brace
                && (ends_at_block
                    || matches!(toks.last(), Some(TokenTree::Punct(p)) if p.as_char() == '!'))
            {
                toks.push(tt);
                rest = next;
                if let Some((TokenTree::Punct(p), after)) = rest.token_tree()
                    && p.as_char() == ';'
                {
                    toks.push(TokenTree::Punct(p));
                    rest = after;
                }
                let semi = matches!(toks.last(), Some(TokenTree::Punct(p)) if p.as_char() == ';');
                return Ok((Split::Stmt(toks.into_iter().collect(), semi), rest));
            }
            match &tt {
                TokenTree::Punct(p) if p.as_char() == '.' => {
                    if prev_joint_dot {
                        match next.token_tree() {
                            Some((TokenTree::Group(g), _)) if g.delimiter() == Delimiter::Brace => {
                                toks.pop();
                                return Ok((Split::Range(toks.into_iter().collect()), next));
                            }
                            Some((TokenTree::Punct(p), _)) if p.as_char() == '|' => {
                                toks.pop();
                                return Ok((Split::RangeClosure(toks.into_iter().collect()), next));
                            }
                            Some((TokenTree::Ident(i), _)) if i == "move" => {
                                toks.pop();
                                return Ok((Split::RangeClosure(toks.into_iter().collect()), next));
                            }
                            _ => {}
                        }
                    }
                    prev_joint_dot = p.spacing() == Spacing::Joint;
                }
                TokenTree::Punct(p) if p.as_char() == ';' => {
                    toks.push(tt);
                    return Ok((Split::Stmt(toks.into_iter().collect(), true), next));
                }
                _ => prev_joint_dot = false,
            }
            toks.push(tt);
            rest = next;
        }
        if toks.is_empty() {
            return Err(cursor.error(EXPECTED_ELEMENT));
        }
        // the block's last statement, without its `;`
        Ok((Split::Stmt(toks.into_iter().collect(), false), rest))
    })
}

/// Rewrites the `ON(signal)` expressions in a function body to `__builder.__signal("signal")`:
/// the signal's arguments,
/// if it fired. Where a `bool` is expected, they become `...is_some()` instead: as the condition
/// of an `if`/`while` or a match guard, or an operand of `&&`, `||` or `!` (so
/// `if ON(pressed) && let Some(v) = .. { .. }` works, and `if let Some(args) = ON(x)` too).
///
/// That's decided on the syntax tree. `ON` in macro arguments, or in a body that doesn't parse
/// (yet), is left to [`rewrite_on_exprs`].
pub fn rewrite_on(body: TokenStream) -> TokenStream {
    use syn::parse::Parser;
    let body = match syn::Block::parse_within.parse2(body.clone()) {
        Ok(mut stmts) => {
            for stmt in &mut stmts {
                OnRewriter.visit_stmt_mut(stmt);
            }
            quote! { #(#stmts)* }
        }
        Err(_) => body,
    };
    rewrite_on_exprs(body)
}

struct OnRewriter;

impl OnRewriter {
    /// `__builder.__signal("x")` (`.is_some()` if `fired`) for an `ON(x)` expression.
    fn signal(expr: &Expr, fired: bool) -> Option<Expr> {
        let Expr::Call(call) = expr else {
            return None;
        };
        let Expr::Path(func) = &*call.func else {
            return None;
        };
        if !func.path.is_ident("ON") || call.args.len() != 1 {
            return None;
        }
        let Expr::Path(arg) = &call.args[0] else {
            return None;
        };
        let name = prop_name(arg.path.get_ident()?);
        // spanned so `#[track_caller]` names this line in the callable's name
        let span = func.path.segments[0].ident.span();
        Some(if fired {
            parse_quote_spanned! {span=> __builder.__signal(#name).is_some() }
        } else {
            parse_quote_spanned! {span=> __builder.__signal(#name) }
        })
    }

    /// An expression a `bool` is expected for.
    fn condition(&mut self, expr: &mut Expr) {
        match expr {
            Expr::Paren(p) => self.condition(&mut p.expr),
            Expr::Binary(b) if matches!(b.op, syn::BinOp::And(_) | syn::BinOp::Or(_)) => {
                self.condition(&mut b.left);
                self.condition(&mut b.right);
            }
            Expr::Unary(u) if matches!(u.op, syn::UnOp::Not(_)) => self.condition(&mut u.expr),
            expr => {
                if let Some(fired) = Self::signal(expr, true) {
                    *expr = fired;
                }
            }
        }
    }
}

impl VisitMut for OnRewriter {
    fn visit_expr_mut(&mut self, expr: &mut Expr) {
        match expr {
            Expr::If(i) => self.condition(&mut i.cond),
            Expr::While(w) => self.condition(&mut w.cond),
            Expr::Binary(b) if matches!(b.op, syn::BinOp::And(_) | syn::BinOp::Or(_)) => {
                self.condition(&mut b.left);
                self.condition(&mut b.right);
            }
            Expr::Unary(u) if matches!(u.op, syn::UnOp::Not(_)) => self.condition(&mut u.expr),
            _ => {}
        }
        if let Some(args) = Self::signal(expr, false) {
            *expr = args;
            return;
        }
        visit_mut::visit_expr_mut(self, expr);
    }

    // a match guard (syn 3 keeps guards in the pattern)
    fn visit_pat_guard_mut(&mut self, guard: &mut syn::PatGuard) {
        self.condition(&mut guard.guard);
        visit_mut::visit_pat_guard_mut(self, guard);
    }
}

/// The token-level `ON(signal)` rewrite: to `__builder.__signal("signal")`, or with
/// `.is_some()` as the whole condition of an `if`. Used for what [`rewrite_on`] can't see.
fn rewrite_on_exprs(ts: TokenStream) -> TokenStream {
    let toks: Vec<TokenTree> = ts.into_iter().collect();
    let mut out: Vec<TokenTree> = Vec::with_capacity(toks.len());
    let mut i = 0;
    while i < toks.len() {
        if let TokenTree::Ident(on) = &toks[i]
            && on == "ON"
            && let Some(TokenTree::Group(g)) = toks.get(i + 1)
            && g.delimiter() == Delimiter::Parenthesis
            && let Some(name) = single_ident(g.stream())
        {
            let whole_if_cond = matches!(out.last(), Some(TokenTree::Ident(k)) if k == "if")
                && matches!(toks.get(i + 2), Some(TokenTree::Group(b)) if b.delimiter() == Delimiter::Brace);
            let name = prop_name(&name);
            // spanned so `#[track_caller]` names this line in the callable's name
            out.extend(quote_spanned! {on.span()=> __builder.__signal(#name) });
            if whole_if_cond {
                out.extend(quote_spanned! {on.span()=> .is_some() });
            }
            i += 2;
            continue;
        }
        match &toks[i] {
            TokenTree::Group(g) => {
                let mut group = Group::new(g.delimiter(), rewrite_on_exprs(g.stream()));
                group.set_span(g.span());
                out.push(group.into());
            }
            tt => out.push(tt.clone()),
        }
        i += 1;
    }
    out.into_iter().collect()
}

/// `ident` or `ident,`
fn single_ident(ts: TokenStream) -> Option<Ident> {
    let mut it = ts.into_iter();
    let TokenTree::Ident(name) = it.next()? else {
        return None;
    };
    match it.next() {
        None => Some(name),
        Some(TokenTree::Punct(p)) if p.as_char() == ',' && it.next().is_none() => Some(name),
        _ => None,
    }
}

/// The removed `let (a, b) = ARGS;` at the start of a call's body, to explain what replaced it.
fn reject_call_args(input: ParseStream) -> syn::Result<()> {
    let fork = input.fork();
    let is_args = fork.parse::<Token![let]>().is_ok() && fork.peek(token::Paren) && {
        let _ = fork.step(|c| c.token_tree().map(|(_, n)| ((), n)).ok_or(c.error("")));
        fork.peek(Token![=]) && fork.peek2(kw::ARGS)
    };
    if is_args {
        return Err(input.error(
            "`let (..) = ARGS;` is gone: take the arguments as the body's parameters, `func(..)..|a, b| { .. }`",
        ));
    }
    Ok(())
}

impl Parse for BlockElem {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if input.peek(kw::INIT) {
            input.parse::<kw::INIT>()?;
            Ok(BlockElem::Init(parse_assignments(input)?))
        } else if input.peek(kw::UPDATE) {
            input.parse::<kw::UPDATE>()?;
            Ok(BlockElem::Update(parse_assignments(input)?))
        } else if input.peek(kw::ON) && input.peek2(token::Paren) {
            // `ON(signal)` expressions were rewritten before this, so it's the old element
            Err(input.error(
                "`ON(signal = handler)` is gone: write the handler as an `if`, \
                 `if ON(signal) { .. }` or `if let Some(args) = ON(signal) { .. }`",
            ))
        } else if input.peek(kw::STATE) && input.peek2(token::Paren) {
            Err(input.error(
                "state is declared with `let`: `let mut name = STATE(initial value);` \
                 (`STATE(name = || init)` is gone)",
            ))
        } else if input.peek(kw::MEMO) {
            input.parse::<kw::MEMO>()?;
            let inner;
            parenthesized!(inner in input);
            let value = parse_all(&inner)?;
            input.parse::<Token![>>]>()?;
            let body = parse_braced_block(input)?;
            end_elem(input)?;
            Ok(BlockElem::Memo { value, body })
        } else if input.peek(kw::BODY) && input.peek2(token::Paren) {
            Err(input.error(
                "`BODY(..)` is gone: take a `body: Body<..>` parameter (the last one) and call it, `body(..)`",
            ))
        } else if input.peek(token::Brace) {
            let body = parse_braced_block(input)?;
            eat_semi(input)?;
            Ok(BlockElem::Scope(body))
        } else if input.peek(Token![if]) {
            let elem = BlockElem::If(input.parse()?);
            eat_semi(input)?;
            Ok(elem)
        } else if input.peek(Token![for]) {
            let for_token = input.parse::<Token![for]>()?;
            let pat = Pat::parse_single(input)?;
            input.parse::<Token![in]>()?;
            let iter = Expr::parse_without_eager_brace(input)?;

            let inner;
            braced!(inner in input);
            let key = if inner.peek(kw::KEY) {
                inner.parse::<kw::KEY>()?;
                let inner2;
                parenthesized!(inner2 in inner);
                let key = parse_all(&inner2)?;
                end_elem(&inner)?;
                Some(key)
            } else {
                None
            };
            let body = inner.parse()?;
            eat_semi(input)?;
            Ok(BlockElem::For {
                for_token,
                pat,
                iter,
                key,
                body,
            })
        } else if input.peek(Token![match]) {
            input.parse::<Token![match]>()?;
            let value = Expr::parse_without_eager_brace(input)?;

            let inner;
            braced!(inner in input);
            let mut arms = vec![];
            while !inner.is_empty() {
                let pat = Pat::parse_multi_with_leading_vert(&inner)?;
                let guard = if inner.peek(Token![if]) {
                    inner.parse::<Token![if]>()?;
                    Some(inner.parse()?)
                } else {
                    None
                };
                inner.parse::<Token![=>]>()?;
                if !inner.peek(token::Brace) {
                    return Err(inner.error("match arms must be blocks: `pat => { .. }`"));
                }
                let body = parse_braced_block(&inner)?;
                if inner.peek(Token![,]) {
                    inner.parse::<Token![,]>()?;
                }
                arms.push(Arm { pat, guard, body });
            }
            eat_semi(input)?;
            Ok(BlockElem::Match { value, arms })
        } else if input.peek(Token![let]) || (input.peek(Token![#]) && is_tree_let(input)) {
            let stmt: syn::Stmt = input.parse()?;
            if let syn::Stmt::Local(local) = &stmt
                && let Some(i) = local.attrs.iter().position(is_tree_attr)
            {
                return view_closure(local, i);
            }
            if let syn::Stmt::Local(local) = &stmt
                && let Some(local_init) = &local.init
                && let Some(init) = state_init(&local_init.expr)
            {
                return let_state(local, init?);
            }
            if let syn::Stmt::Local(local) = &stmt
                && let Some(local_init) = &local.init
                && let Some(range) = range_init(&local_init.expr)
            {
                if local_init.diverge.is_some() {
                    return Err(syn::Error::new_spanned(
                        &local.pat,
                        "`let .. else` can't be used here",
                    ));
                }
                let pat = local.pat.clone();
                return Ok(match range? {
                    RangeInit::Node(ty, body) => BlockElem::LetNode { pat, ty, body },
                    RangeInit::Call(call, params, body) => BlockElem::LetCall {
                        pat,
                        call,
                        params,
                        body,
                    },
                });
            }
            Ok(BlockElem::Stmt {
                tokens: stmt.into_token_stream(),
                semi: true,
            })
        } else if could_start_item(input)
            && let Some(item) = parse_item(input)
        {
            // a nested `fn`, `struct`, `impl`, `use`, .. (as Rust allows in a function body)
            Ok(BlockElem::Stmt {
                tokens: item.into_token_stream(),
                semi: true,
            })
        } else {
            let (head, closure) = match split_statement(input)? {
                Split::Stmt(tokens, semi) => return Ok(BlockElem::Stmt { tokens, semi }),
                Split::Range(head) => (head, false),
                Split::RangeClosure(head) => (head, true),
            };
            let elem = match parse2::<Expr>(head.clone()) {
                Ok(call @ (Expr::Call(_) | Expr::MethodCall(_))) if closure => {
                    let closure: syn::ExprClosure = input.parse()?;
                    let (params, body) = closure_body(&closure)?;
                    BlockElem::Call { call, params, body }
                }
                Ok(other) if closure => {
                    return Err(syn::Error::new_spanned(
                        other,
                        "only tree function calls take arguments in their body (`func(..)..|a| { .. }`)",
                    ));
                }
                Ok(Expr::Path(p)) if p.qself.is_none() => {
                    let inner;
                    braced!(inner in input);
                    let body = inner.parse()?;
                    BlockElem::Node { ty: p.path, body }
                }
                Ok(call @ (Expr::Call(_) | Expr::MethodCall(_))) => {
                    let inner;
                    braced!(inner in input);
                    let CallBlock { params, body } = inner.parse()?;
                    BlockElem::Call { call, params, body }
                }
                Ok(other) => {
                    return Err(syn::Error::new_spanned(
                        other,
                        "expected a node type (`Label..{}`) or a tree function call (`func(..)..{}`) before `..{`",
                    ));
                }
                Err(e) => return Err(e),
            };
            end_elem(input)?;
            Ok(elem)
        }
    }
}

/// The argument of `STATE(init)`, if `expr` is that.
fn state_init(expr: &Expr) -> Option<syn::Result<Expr>> {
    let Expr::Call(call) = expr else {
        return None;
    };
    let Expr::Path(func) = &*call.func else {
        return None;
    };
    if !func.path.is_ident("STATE") {
        return None;
    }
    Some(match call.args.len() {
        1 => Ok(call.args[0].clone()),
        _ => Err(syn::Error::new_spanned(
            &call.args,
            "`STATE(..)` takes the state's initial value",
        )),
    })
}

/// A call's `{ .. }` body (rejecting the removed `let (a, b) = ARGS;` at its start).
struct CallBlock {
    params: CallParams,
    body: Block,
}

impl Parse for CallBlock {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        reject_call_args(input)?;
        Ok(CallBlock {
            params: CallParams::None,
            body: input.parse()?,
        })
    }
}

/// The tokens of a closure's body, without the braces if it's a block, to be parsed as elements.
/// (rustfmt takes the braces off single-expression closure bodies.)
pub fn body_tokens(expr: &Expr) -> TokenStream {
    match expr {
        Expr::Block(b) if b.attrs.is_empty() && b.label.is_none() => b
            .block
            .stmts
            .iter()
            .map(ToTokens::to_token_stream)
            .collect(),
        expr => expr.to_token_stream(),
    }
}

/// The parameters and body of a call's `|a, b| { .. }` body.
fn closure_body(closure: &syn::ExprClosure) -> syn::Result<(CallParams, Block)> {
    if !matches!(closure.output, syn::ReturnType::Default) || !closure.attrs.is_empty() {
        return Err(syn::Error::new_spanned(
            closure,
            "a call's body takes parameters only: `func(..)..|a, b| { .. }`",
        ));
    }
    let params = CallParams::Closure {
        capture: closure.capture,
        pats: closure.inputs.iter().cloned().collect(),
    };
    Ok((params, parse2(body_tokens(&closure.body))?))
}

enum RangeInit {
    Node(syn::Path, Block),
    Call(Expr, CallParams, Block),
}

/// `Node..{ body }`, `call(..)..{ body }` or `call(..)..|a| body`, if `expr` is one of those.
fn range_init(expr: &Expr) -> Option<syn::Result<RangeInit>> {
    let Expr::Range(range) = expr else {
        return None;
    };
    let (Some(start), syn::RangeLimits::HalfOpen(_), Some(end)) =
        (&range.start, &range.limits, &range.end)
    else {
        return None;
    };
    let is_call = matches!(&**start, Expr::Call(_) | Expr::MethodCall(_));
    match (&**start, &**end) {
        (Expr::Path(p), Expr::Block(_)) if p.qself.is_none() => Some(
            // syn parsed the body as Rust; parse it again as elements
            parse2::<Block>(body_tokens(end)).map(|body| RangeInit::Node(p.path.clone(), body)),
        ),
        (call, Expr::Block(_)) if is_call => Some(
            parse2::<CallBlock>(body_tokens(end))
                .map(|CallBlock { params, body }| RangeInit::Call(call.clone(), params, body)),
        ),
        (call, Expr::Closure(c)) if is_call => {
            Some(closure_body(c).map(|(params, body)| RangeInit::Call(call.clone(), params, body)))
        }
        _ => None,
    }
}

/// Whether an item (`fn`, `struct`, `impl`, ..) could start here; checked before trying to
/// parse one, which most elements aren't.
fn could_start_item(input: ParseStream) -> bool {
    input.peek(Token![fn])
        || input.peek(Token![struct])
        || input.peek(Token![enum])
        || input.peek(Token![union])
        || input.peek(Token![impl])
        || input.peek(Token![trait])
        || input.peek(Token![auto])
        || input.peek(Token![use])
        || input.peek(Token![const])
        || input.peek(Token![static])
        || input.peek(Token![type])
        || input.peek(Token![mod])
        || input.peek(Token![extern])
        || input.peek(Token![pub])
        || input.peek(Token![unsafe])
        || input.peek(Token![async])
        || input.peek(Token![#])
        || (input.peek(kw::macro_rules) && input.peek2(Token![!]))
}

/// Parses an item, if there is one (not a block like `unsafe { .. }` or `async move { .. }`).
fn parse_item(input: ParseStream) -> Option<syn::Item> {
    let fork = input.fork();
    match fork.parse::<syn::Item>() {
        Ok(syn::Item::Verbatim(_)) | Err(_) => None,
        Ok(item) => {
            input.advance_to(&fork);
            Some(item)
        }
    }
}

fn is_tree_attr(attr: &syn::Attribute) -> bool {
    attr.path()
        .segments
        .last()
        .is_some_and(|s| s.ident == "tree")
}

/// Whether the outer attributes coming up belong to a `#[tree] let`.
fn is_tree_let(input: ParseStream) -> bool {
    let fork = input.fork();
    fork.call(syn::Attribute::parse_outer)
        .is_ok_and(|attrs| attrs.iter().any(is_tree_attr) && fork.peek(Token![let]))
}

/// Whether `ty` is (some path to) `Body<..>`, the type of a body parameter.
pub fn is_body_type(ty: &syn::Type) -> bool {
    matches!(ty, syn::Type::Path(p) if p.qself.is_none()
        && p.path.segments.last().is_some_and(|s| s.ident == "Body"))
}

/// `Body<'a, N, A>` written out: `&'a mut dyn FnMut(Builder<N>, A)`. It's the same type, but
/// lifetimes elided in `A` (like the one in `&str`) are then elided inside the `FnMut(..)`, which
/// makes them higher-ranked: the function can give its body borrows of its own locals. Through
/// the alias, they'd be lifetimes of the function's signature, outliving the whole call.
pub fn expand_body_type(ty: &syn::Type) -> Option<syn::Type> {
    let syn::Type::Path(p) = ty else {
        return None;
    };
    let seg = p.path.segments.last()?;
    if p.qself.is_some() || seg.ident != "Body" {
        return None;
    }
    let mut lifetime = None;
    let mut types = vec![];
    match &seg.arguments {
        syn::PathArguments::None => {}
        syn::PathArguments::AngleBracketed(a) => {
            for arg in &a.args {
                match arg {
                    syn::GenericArgument::Lifetime(l) if lifetime.is_none() => {
                        lifetime = Some(l.clone())
                    }
                    syn::GenericArgument::Type(t) => types.push(t.clone()),
                    _ => return None,
                }
            }
        }
        syn::PathArguments::Parenthesized(_) => return None,
    }
    if types.len() > 2 {
        return None;
    }
    let node = types
        .first()
        .map_or_else(|| quote! { ::echo::AnyNode }, |t| quote! { #t });
    let args = types
        .get(1)
        .map_or_else(|| quote! { () }, |t| quote! { #t });
    Some(parse_quote_spanned! {ty.span()=>
        & #lifetime mut dyn ::core::ops::FnMut(::echo::Builder<#node>, #args)
    })
}

/// `#[tree] let name = |..| { .. };`
fn view_closure(local: &syn::Local, tree_attr: usize) -> syn::Result<BlockElem> {
    let mut attrs = local.attrs.clone();
    let attr = attrs.remove(tree_attr);
    let usable = match &attr.meta {
        syn::Meta::Path(_) => None,
        _ => attr.parse_args::<TreeAttr>()?.usable,
    };
    let closure = match &local.init {
        Some(syn::LocalInit {
            expr,
            diverge: None,
            ..
        }) => match &**expr {
            Expr::Closure(c) => c.clone(),
            other => {
                return Err(syn::Error::new_spanned(
                    other,
                    "`#[tree]` on a `let` needs a closure: `#[tree] let name = |..| { .. };`",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                &local.pat,
                "`#[tree]` on a `let` needs a closure: `#[tree] let name = |..| { .. };`",
            ));
        }
    };
    Ok(BlockElem::ViewClosure {
        attrs,
        pat: local.pat.clone(),
        usable,
        closure,
    })
}

/// Rewrites calls of the body parameter `name(a, b)` to pass it the builder and its arguments,
/// in a scope of their own: `{ let __scope = ..; name(__builder.clone().upcast(), (a, b)); }`
/// (one argument is passed as is, none as `()`).
///
/// Nested `fn`s are left alone (a nested `#[tree] fn` rewrites its own), and so are calls that
/// were already rewritten (a `#[tree]` closure's pass runs after its function's).
pub fn rewrite_body_calls(ts: TokenStream, name: &Ident) -> TokenStream {
    use syn::parse::Parser;
    let toks: Vec<TokenTree> = ts.into_iter().collect();
    let mut out: Vec<TokenTree> = Vec::with_capacity(toks.len());
    let mut i = 0;
    while i < toks.len() {
        // a nested `fn name..`, up to its body (not a `fn(..)` pointer type)
        if matches!(&toks[i], TokenTree::Ident(k) if k == "fn")
            && matches!(toks.get(i + 1), Some(TokenTree::Ident(_)))
        {
            while i < toks.len() {
                let tt = toks[i].clone();
                i += 1;
                let end = matches!(&tt, TokenTree::Group(g) if g.delimiter() == Delimiter::Brace)
                    || matches!(&tt, TokenTree::Punct(p) if p.as_char() == ';');
                out.push(tt);
                if end {
                    break;
                }
            }
            continue;
        }
        if let TokenTree::Ident(id) = &toks[i]
            && id == name
            && let Some(TokenTree::Group(g)) = toks.get(i + 1)
            && g.delimiter() == Delimiter::Parenthesis
            // already rewritten
            && !matches!(g.stream().into_iter().next(), Some(TokenTree::Ident(b)) if b == "__builder")
            // not a method or path segment of the same name, or a fn declaration
            && !matches!(out.last(), Some(TokenTree::Punct(p)) if p.as_char() == '.' || p.as_char() == ':')
            && !matches!(out.last(), Some(TokenTree::Ident(k)) if k == "fn")
            && let Ok(args) = Punctuated::<Expr, Token![,]>::parse_terminated
                .parse2(rewrite_body_calls(g.stream(), name))
        {
            let args = match args.len() {
                0 => quote! { () },
                1 => args[0].to_token_stream(),
                _ => {
                    let args = args.iter();
                    quote! { (#(#args),*) }
                }
            };
            // a scope of its own, like a call: the body can return early
            out.extend(quote_spanned! {id.span()=>
                {
                    let __scope = __builder.__scope();
                    #id(__builder.clone().upcast(), #args);
                }
            });
            i += 2;
            continue;
        }
        match &toks[i] {
            TokenTree::Group(g) => {
                let mut group = Group::new(g.delimiter(), rewrite_body_calls(g.stream(), name));
                group.set_span(g.span());
                out.push(group.into());
            }
            tt => out.push(tt.clone()),
        }
        i += 1;
    }
    out.into_iter().collect()
}

/// `let [mut] name[: T] = STATE(init);`
fn let_state(local: &syn::Local, init: Expr) -> syn::Result<BlockElem> {
    let unsupported = || {
        syn::Error::new_spanned(
            &local.pat,
            "`STATE` needs a plain name: `let name = STATE(..);` or `let mut name: T = STATE(..);`",
        )
    };
    if local.init.as_ref().is_some_and(|i| i.diverge.is_some()) {
        return Err(unsupported());
    }
    let (pat, ty) = match &local.pat {
        Pat::Type(t) => (&*t.pat, Some(t.ty.clone())),
        pat => (pat, None),
    };
    let Pat::Ident(syn::PatIdent {
        by_ref: None,
        mutability,
        ident,
        subpat: None,
        ..
    }) = pat
    else {
        return Err(unsupported());
    };
    Ok(BlockElem::LetState {
        mutability: *mutability,
        name: ident.clone(),
        ty,
        init,
    })
}

/// Skips the rest of a broken element, up to and including the next `;`.
fn skip_element(input: ParseStream) {
    let _ = input.step(|cursor| {
        let mut rest = *cursor;
        while let Some((tt, next)) = rest.token_tree() {
            rest = next;
            if matches!(&tt, TokenTree::Punct(p) if p.as_char() == ';') {
                break;
            }
        }
        Ok(((), rest))
    });
}

impl Parse for Block {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut elems = vec![];
        while !input.is_empty() {
            let fork = input.fork();
            match fork.parse::<BlockElem>() {
                Ok(elem) => {
                    input.advance_to(&fork);
                    elems.push(Ok(elem));
                }
                Err(e) => {
                    elems.push(Err(e));
                    skip_element(input);
                }
            }
        }
        Ok(Block { elems })
    }
}

/// A signal's name as a string (`r#type` is `"type"`).
fn prop_name(name: &Ident) -> LitStr {
    LitStr::new(&name.unraw().to_string(), name.span())
}

/// The setter call for one assignment, on the `__node` handle (spanned at the name, so a
/// property the element doesn't have is reported there).
fn gen_assignment(a: Assignment) -> TokenStream {
    let setter = format_ident!("set_{}", a.name.unraw(), span = a.name.span());
    let value = a.value;
    let args = a.args.into_iter().flatten();
    quote! { __node.#setter(#(#args,)* #value); }
}

/// `INIT`/`UPDATE`'s assignments, with the node's handle.
fn gen_assignments(list: Punctuated<Assignment, Token![,]>) -> TokenStream {
    let sets: TokenStream = list.into_iter().map(gen_assignment).collect();
    quote! {
        #[allow(unused_mut)]
        let mut __node = __builder.node();
        #sets
    }
}

/// Whether generated code takes no positions in its scope: it creates no nodes, state, memos,
/// branches etc., so it doesn't need a scope of its own. Only the `__builder` methods that never
/// take a position may appear (anything else, including hand-written internals, doesn't count).
fn takes_no_position(ts: &TokenStream) -> bool {
    let toks: Vec<TokenTree> = ts.clone().into_iter().collect();
    toks.iter().enumerate().all(|(i, tt)| match tt {
        TokenTree::Ident(id) if id == "__body" => false,
        TokenTree::Ident(id) if id == "__builder" => {
            matches!(toks.get(i + 1), Some(TokenTree::Punct(p)) if p.as_char() == '.')
                && matches!(toks.get(i + 2), Some(TokenTree::Ident(m)) if {
                    let m = m.to_string();
                    matches!(m.as_str(), "__signal" | "init" | "node" | "path" | "forcing_memos")
                })
        }
        TokenTree::Group(g) => takes_no_position(&g.stream()),
        _ => true,
    })
}

/// Lowers an `if`/`else if`/`else` chain. Each branch is a scope of its own (so switching
/// branches can't shift the identity of what comes after), unless no branch takes a position:
/// then it's a plain `if`, which is what event handlers (`if ON(pressed) { .. }`) usually are.
fn gen_if(if_elem: IfElem) -> TokenStream {
    let mut branches = vec![];
    let mut next = Some(if_elem);
    let mut last_else = None;
    while let Some(IfElem {
        cond,
        body,
        else_expr,
    }) = next.take()
    {
        branches.push((cond, gen_block(body)));
        match else_expr {
            Some(Left(elif)) => next = Some(*elif),
            Some(Right(block)) => last_else = Some(gen_block(block)),
            None => {}
        }
    }
    let plain = branches.iter().all(|(_, b)| takes_no_position(b))
        && last_else.iter().all(takes_no_position);
    let scope = |idx: usize| {
        let idx = idx as u64;
        (!plain).then(|| quote! { let __scope = __builder.__keyed(__slot, #idx); })
    };
    let mut chain = quote! {};
    for (idx, (cond, body)) in branches.iter().enumerate() {
        if idx > 0 {
            chain.extend(quote! { else });
        }
        let scope = scope(idx);
        chain.extend(quote! { if #cond { #scope #body } });
    }
    if let Some(body) = last_else {
        let scope = scope(branches.len());
        chain.extend(quote! { else { #scope #body } });
    }
    if plain {
        chain
    } else {
        quote! {
            {
                let __slot = __builder.__slot();
                #chain
            }
        }
    }
}

/// The body closure passed to a call: `&mut |__builder, args| { body }`, with the arguments
/// taken apart as the call's body asks (`..|a, b|`).
fn gen_call(mut call: Expr, params: CallParams, body: Block) -> TokenStream {
    let body = gen_block(body);
    let (capture, args) = match params {
        CallParams::None => (None, quote! { _ }),
        CallParams::Closure { capture, pats } => {
            let args = match pats.len() {
                0 => quote! { _ },
                1 => pats[0].to_token_stream(),
                // `|a: A, b|` becomes `(a, b): (A, _)`: no types inside a tuple pattern
                _ => {
                    let untyped = pats.iter().map(|p| match p {
                        Pat::Type(t) => &*t.pat,
                        p => p,
                    });
                    if pats.iter().any(|p| matches!(p, Pat::Type(_))) {
                        let types = pats.iter().map(|p| match p {
                            Pat::Type(t) => t.ty.to_token_stream(),
                            _ => quote! { _ },
                        });
                        quote! { (#(#untyped),*): (#(#types),*) }
                    } else {
                        quote! { (#(#untyped),*) }
                    }
                }
            };
            (capture, args)
        }
    };
    // spanned so "can't be used in this node" errors point at the function name
    let func_span = match &call {
        Expr::Call(c) => c.func.span(),
        Expr::MethodCall(c) => c.method.span(),
        _ => unreachable!(),
    };
    let builder_arg: Expr = parse_quote_spanned! {func_span=> __builder.clone().upcast() };
    let body_arg: Expr = parse_quote! {
        &mut #capture |__builder, #args| {
            #body
        }
    };
    let call_args = match &mut call {
        Expr::Call(c) => &mut c.args,
        Expr::MethodCall(c) => &mut c.args,
        _ => unreachable!(),
    };
    call_args.insert(0, builder_arg);
    call_args.push(body_arg);
    call.to_token_stream()
}

/// `#[tree] let name = |..| { .. };`: the closure gets the builder as its first parameter and a
/// body as its last (unless it declares one), like a tree function.
fn gen_view_closure(
    attrs: Vec<syn::Attribute>,
    pat: Pat,
    usable: Option<syn::Path>,
    mut closure: syn::ExprClosure,
) -> TokenStream {
    let typed = |name: &str, ty: syn::Type| {
        Pat::Type(syn::PatType {
            attrs: vec![],
            pat: Box::new(Pat::Ident(syn::PatIdent {
                attrs: vec![],
                by_ref: None,
                mutability: None,
                ident: Ident::new(name, proc_macro2::Span::call_site()),
                subpat: None,
            })),
            colon_token: Default::default(),
            ty: Box::new(ty),
        })
    };
    // written out (see `expand_body_type`), keeping the `Body<..>` as written for its import
    let mut written_body = None;
    let body_name = match closure.inputs.last_mut() {
        Some(Pat::Type(t)) if is_body_type(&t.ty) => match &*t.pat {
            Pat::Ident(i) => {
                if let Some(ty) = expand_body_type(&t.ty) {
                    written_body = Some(mem::replace(&mut *t.ty, ty));
                }
                Some(i.ident.clone())
            }
            _ => None,
        },
        _ => None,
    };
    // written out, it can look complex to clippy
    let allow = written_body
        .is_some()
        .then(|| quote! { #[allow(clippy::type_complexity)] });
    let written_body = written_body.map(|ty| quote! { let _: ::core::marker::PhantomData<#ty>; });
    let body_name = body_name.unwrap_or_else(|| {
        closure
            .inputs
            .push(typed("__body", parse_quote! { ::echo::Body<'_> }));
        format_ident!("__body")
    });
    let usable = usable.map_or_else(|| quote! { ::echo::AnyNode }, |u| quote! { #u });
    closure.inputs.insert(
        0,
        typed("__builder", parse_quote! { ::echo::Builder<#usable> }),
    );
    let tokens = rewrite_body_calls(body_tokens(&closure.body), &body_name);
    let block = match parse2::<Block>(tokens) {
        Ok(block) => gen_body(block),
        Err(e) => e.to_compile_error(),
    };
    *closure.body = Expr::Verbatim(quote! { { #written_body #block } });
    quote! { #(#attrs)* #allow let #pat = #closure; }
}

/// Lowers a block of elements to plain statements. Each scope is a Rust block holding a guard
/// (`__scope`) that closes it on drop, and `__builder` is shadowed by the inner scope's builder.
pub fn gen_block(block: Block) -> TokenStream {
    gen_elems(block, false)
}

/// Like [`gen_block`], for a function or closure body: a last statement without `;` stays the
/// tail expression, its value.
pub fn gen_body(block: Block) -> TokenStream {
    gen_elems(block, true)
}

fn gen_elems(block: Block, tail: bool) -> TokenStream {
    let mut out = quote! {};
    let count = block.elems.len();
    for (i, elem) in block.elems.into_iter().enumerate() {
        let is_tail = tail && i + 1 == count;
        let elem = match elem {
            Ok(elem) => elem,
            Err(e) => {
                out.extend(e.to_compile_error());
                continue;
            }
        };
        match elem {
            BlockElem::Node { ty, body } => {
                let body = gen_block(body);
                out.extend(quote! {
                    crate::__echo_deny!(#ty);
                    {
                        let (__scope, __builder) = __builder.__child::<#ty>();
                        #body
                    }
                });
            }
            BlockElem::LetNode { pat, ty, body } => {
                let body = gen_block(body);
                out.extend(quote! {
                    crate::__echo_deny!(#ty);
                    let #pat = {
                        let (__scope, __builder) = __builder.__child::<#ty>();
                        #body
                        __builder.node()
                    };
                });
            }
            BlockElem::Init(list) => {
                let sets = gen_assignments(list);
                out.extend(quote! {
                    if __builder.init() {
                        #sets
                    }
                });
            }
            BlockElem::Update(list) => {
                let sets = gen_assignments(list);
                out.extend(quote! { { #sets } });
            }
            BlockElem::LetState {
                mutability,
                name,
                ty,
                init,
            } => {
                // a type annotation names the value's type
                let ty = ty.map(|ty| quote! { : ::echo::StateMut<#ty> });
                out.extend(quote! {
                    let #mutability #name #ty = __builder.__state_mut(|| #init);
                });
            }
            BlockElem::Scope(body) => {
                let body = gen_block(body);
                out.extend(quote! { { #body } });
            }
            BlockElem::Stmt { tokens, semi } => {
                out.extend(tokens);
                if !semi && !is_tail {
                    out.extend(quote! { ; });
                }
            }
            // A call is a scope of its own: however many positions the function takes (it can
            // return early), it's one position here, so it can't shift what comes after.
            BlockElem::Call { call, params, body } => {
                let span = call.span();
                let call = gen_call(call, params, body);
                out.extend(quote_spanned! {span=>
                    {
                        let __scope = __builder.__scope();
                        #call;
                    }
                });
            }
            BlockElem::LetCall {
                pat,
                call,
                params,
                body,
            } => {
                let call = gen_call(call, params, body);
                out.extend(quote! {
                    let #pat = {
                        let __scope = __builder.__scope();
                        #call
                    };
                });
            }
            BlockElem::ViewClosure {
                attrs,
                pat,
                usable,
                closure,
            } => out.extend(gen_view_closure(attrs, pat, usable, closure)),
            BlockElem::If(if_elem) => out.extend(gen_if(if_elem)),
            BlockElem::For {
                for_token,
                pat,
                iter,
                key,
                body,
            } => {
                let body = gen_block(body);
                match key {
                    Some(key) => out.extend(quote! {
                        {
                            let __slot = __builder.__slot();
                            for #pat in #iter {
                                let __scope = __builder.__keyed(__slot, #key);
                                #body
                            }
                        }
                    }),
                    None => {
                        // Nothing in it has an identity, so it's just a loop. If something
                        // does, it needs a key: index identity would be a silent footgun.
                        if !takes_no_position(&body) {
                            out.extend(
                                syn::Error::new(
                                    for_token.span,
                                    "this loop creates nodes (or state, memos, ..), so its body needs to start with `KEY(expr);`, \
                                     giving each item a stable identity (for identity by index: `for (i, x) in xs.iter().enumerate() { KEY(i); .. }`)",
                                )
                                .to_compile_error(),
                            );
                        }
                        // still emitted, so the body stays checked (and analyzable)
                        out.extend(quote! { for #pat in #iter { #body } });
                    }
                }
            }
            BlockElem::Match { value, arms } => {
                // like `if`: each arm is a scope, unless none of them takes a position
                let arms: Vec<_> = arms
                    .into_iter()
                    .map(|Arm { pat, guard, body }| (pat, guard, gen_block(body)))
                    .collect();
                let plain = arms.iter().all(|(_, _, body)| takes_no_position(body));
                let arms = arms
                    .into_iter()
                    .enumerate()
                    .map(|(idx, (pat, guard, body))| {
                        let guard = guard.map(|g| quote! { if #g });
                        let idx = idx as u64;
                        let scope = (!plain)
                            .then(|| quote! { let __scope = __builder.__keyed(__slot, #idx); });
                        quote! {
                            #pat #guard => {
                                #scope
                                #body
                            }
                        }
                    });
                if plain {
                    out.extend(quote! { match #value { #(#arms)* } });
                } else {
                    out.extend(quote! {
                        {
                            let __slot = __builder.__slot();
                            match #value {
                                #(#arms)*
                            }
                        }
                    });
                }
            }
            BlockElem::Memo { value, body } => {
                let body = gen_block(body);
                out.extend(quote! {
                    {
                        let __scope = __builder.__memo(#value);
                        if __scope.__run() {
                            #body
                        }
                    }
                });
            }
        }
    }
    out
}
