use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    Attribute, FnArg, Ident, PatType, Result, Signature, Visibility,
    parse::{Parse, ParseStream},
    parse_quote,
};

use crate::tree::{Block, TreeAttr, gen_body};

mod tree;

struct RawFn {
    attrs: Vec<Attribute>,
    vis: Visibility,
    sig: Signature,
    body: TokenStream,
}

impl Parse for RawFn {
    fn parse(input: ParseStream) -> Result<Self> {
        let attrs = input.call(Attribute::parse_outer)?;
        let vis = input.parse()?;
        let sig = input.parse()?;

        let body;
        syn::braced!(body in input);
        let body: TokenStream = body.parse()?;

        Ok(RawFn {
            attrs,
            vis,
            sig,
            body,
        })
    }
}

/// Turns a function written in the tree DSL into one that builds nodes.
///
/// ```text
/// #[tree]                          usable inside any node
/// #[tree(in PanelContainer)]       only inside a PanelContainer (checked at compile time)
/// fn card(title: &str, body: Body<VBoxContainer, i32>) -> bool { .. }
/// ```
///
/// If the last parameter is a `Body<N, Args>`, it's the body the caller writes after the call
/// (`card("Hi")..|n| { .. }`), placed with `body(args)` inside an `N` node. `Args` is a type, a
/// tuple for several arguments, or left out for none. The function returns its own value (its
/// last expression), which the caller can keep: `let v = card("Hi")..{ .. };`.
///
/// # Elements
///
/// ```text
/// Label..{ .. };                       a child node (any path: `my::Widget..{}`)
/// let a = Label..{ .. };               ..and its `Gd<Label>`, for the code after it
/// INIT(text = "a", visible = false);   set properties when the node is created
/// UPDATE(text = s.name.clone());       set properties on every run
/// INIT(theme(color, font_color) = c);  theme overrides: color/constant/font/font_size/icon/stylebox
/// if ON(pressed) { .. }               true during the rerun caused by the signal
/// if let Some(args) = ON(value_changed) { .. }   ..with the signal's arguments
/// let mut x = STATE(init);            state kept at this place in the tree; `x` derefs to it
///                                      (`*x += 1`, `x.push(..)`); `init` runs once, on creation
/// MEMO(value) >> { .. };               skips the block while `value` stays equal (PartialEq)
/// if c { .. } else { .. }              each branch has its own identity
/// match v { pat if g => { .. } }       each arm has its own identity
/// for x in xs { KEY(x.id); .. }        each item's identity follows its key (a loop that
///                                      creates nothing with an identity needs no `KEY`)
/// f(args)..{ .. };                     call a tree function with a body,
/// f(args)..|a, b| { .. };              ..taking the arguments the function gives it
/// let v = f(args)..{ .. };             ..keeping what it returns
/// body(a, b);                          place the caller's body here (the `Body` parameter)
/// #[tree] let row = |..| { .. };       a tree closure, called like a function: `row(..)..{};`
/// let x = ..;  /  any_stmt;            plain Rust statements, and items (`fn`, `struct`, `impl`, ..)
/// { .. }                               a block scope, as in Rust (with elements in it too)
/// ```
///
/// `STATE`'s value is dropped when its element stops running, and comes back fresh.
///
/// `ON(signal)` is an `Option<args>` expression for the current node's signal and can be used
/// anywhere; as an `if`'s whole condition it means "fired". The signal stays connected while
/// its `ON` keeps being evaluated. Check a node's events before the `UPDATE`s that show what
/// they change: code runs top to bottom, so otherwise the run caused by the signal first sets
/// the node back to the old value. The last element of a block doesn't need a `;`.
///
/// Elements are identified by their position in the code (and by branch/key), so nodes,
/// state and signal connections persist across runs as long as their element keeps running.
/// Inside the function, `__builder` is the [`Builder`](../echo/struct.Builder.html) for the
/// current node (`__builder.node()`, `__builder.init()`, ..).
///
/// Every node goes through `crate::__echo_deny!(Type)` first, which the crate must define
/// (e.g. to reject some node types with `compile_error!`).
#[proc_macro_attribute]
pub fn tree(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let attr = match syn::parse::<TreeAttr>(attr) {
        Ok(a) => a,
        Err(e) => return e.to_compile_error().into(),
    };
    let RawFn {
        attrs,
        vis,
        mut sig,
        body,
    } = match syn::parse::<RawFn>(item) {
        Ok(f) => f,
        Err(e) => return e.to_compile_error().into(),
    };
    let body = tree::rewrite_on(body);
    let param = |name: &str, ty: syn::Type| {
        FnArg::Typed(PatType {
            attrs: vec![],
            pat: Box::new(syn::Pat::Ident(syn::PatIdent {
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
    let usable = attr
        .usable
        .map_or_else(|| quote! { ::godot::classes::Node }, |u| quote! { #u });
    let has_self = matches!(sig.inputs.first(), Some(FnArg::Receiver(_)));
    sig.inputs.insert(
        if has_self { 1 } else { 0 },
        param("__builder", parse_quote! { ::echo::Builder<#usable> }),
    );

    // the body parameter: the last one, if it's a `Body`, else an unused one
    // (and the `Body<..>` as written, so its import counts as used and the editor knows it)
    let mut written_body = None;
    let declared = match sig.inputs.last_mut() {
        Some(FnArg::Typed(t)) if tree::is_body_type(&t.ty) => match &*t.pat {
            syn::Pat::Ident(i) => {
                // written out, so it can be given borrows of the function's locals
                if let Some(ty) = tree::expand_body_type(&t.ty) {
                    written_body = Some(std::mem::replace(&mut *t.ty, ty));
                }
                Some(i.ident.clone())
            }
            _ => None,
        },
        _ => None,
    };
    // written out, it can look complex to clippy (which checks a whole signature at once)
    let allow = written_body
        .is_some()
        .then(|| quote! { #[allow(clippy::type_complexity)] });
    let written_body = written_body.map(|ty| quote! { let _: ::core::marker::PhantomData<#ty>; });
    let body_name = declared.unwrap_or_else(|| {
        sig.inputs
            .push(param("__body", parse_quote! { ::echo::Body<'_> }));
        Ident::new("__body", proc_macro2::Span::call_site())
    });

    // Element errors become `compile_error!`s in place; the signature is always emitted so
    // callers don't drown in follow-up errors.
    let block = match syn::parse2::<Block>(tree::rewrite_body_calls(body, &body_name)) {
        Ok(block) => gen_body(block),
        Err(e) => e.to_compile_error(),
    };
    quote! {
        #(#attrs)*
        #allow
        #vis #sig {
            #written_body
            #block
        }
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::tree::{Block, gen_block};

    fn expand(src: &str) -> String {
        let ts: proc_macro2::TokenStream = src.parse().unwrap();
        let block: Block = syn::parse2(super::tree::rewrite_on(ts)).unwrap();
        gen_block(block).to_string()
    }

    fn errors(src: &str) -> Vec<String> {
        let out = expand(src);
        out.split("compile_error ! {")
            .skip(1)
            .map(|e| e.split('}').next().unwrap().trim().to_string())
            .collect()
    }

    #[test]
    fn broken_element_does_not_take_down_the_rest() {
        let out = expand(
            r#"
            Label..{ INIT(text = ); };
            Button..{ INIT(text = "ok"); };
            "#,
        );
        assert!(out.contains("compile_error"));
        assert!(out.contains(r#"__set_prop ("text" , "ok")"#), "{out}");
    }

    #[test]
    fn for_without_key() {
        // creating nodes, it needs a key
        let errs = errors("for x in y { Label..{}; }");
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("KEY"), "{errs:?}");
        assert_eq!(errors("for x in y { let s = STATE(0); }").len(), 1);
        assert_eq!(errors("for x in y { f()..{}; }").len(), 1);
        // otherwise it's a plain loop
        let out = expand("for x in y { total += x; if ON(p) { break } UPDATE(a = x) }");
        assert!(!out.contains("compile_error"), "{out}");
        assert!(!out.contains("__slot"), "{out}");
        assert!(out.starts_with("for x in y { total += x ;"), "{out}");
        // keyed loops are unchanged
        assert!(expand("for x in y { KEY(x); Label..{}; }").contains("__keyed (__slot , x)"));
    }

    #[test]
    fn not_a_node_or_call() {
        let errs = errors("a.b..{};");
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("node type"), "{errs:?}");
    }

    #[test]
    fn statements_and_paths() {
        let out = expand(
            r#"
            let r = 0..5;
            x += 1;
            godot::classes::Label..{};
            "#,
        );
        assert!(!out.contains("compile_error"), "{out}");
        assert!(out.contains("let r = 0 .. 5 ;"), "{out}");
        assert!(
            out.contains("__child :: < godot :: classes :: Label >"),
            "{out}"
        );
    }

    #[test]
    fn trailing_commas() {
        // what rustfmt produces once these get long enough to be split over lines
        let out = expand(
            r#"
            let v = STATE(
                {
                    //
                },
            );
            MEMO(
                (a, b),
            ) >> {};
            for x in xs {
                KEY(
                    x.id,
                );
            }
            INIT(theme(color, font_color,) = c,);
            "#,
        );
        assert!(!out.contains("compile_error"), "{out}");
    }

    #[test]
    fn on_expressions() {
        let out = expand(
            r#"
            if ON(pressed) { a() }
            if let Some(args) = ON(value_changed,) { b(args) } else if ON(other) {}
            let fired = ON(pressed).is_some();
            { foo(ON(x)); }
            "#,
        );
        assert!(!out.contains("compile_error"), "{out}");
        assert!(!out.contains("ON"), "{out}");
        assert!(
            out.contains(r#"if __builder . __signal ("pressed") . is_some ()"#),
            "{out}"
        );
        assert!(
            out.contains(r#"Some (args) = __builder . __signal ("value_changed") {"#),
            "{out}"
        );
        assert!(
            out.contains(r#"if __builder . __signal ("other") . is_some ()"#),
            "{out}"
        );
        assert!(
            out.contains(r#"let fired = __builder . __signal ("pressed") . is_some () ;"#),
            "{out}"
        );
        assert!(out.contains(r#"foo (__builder . __signal ("x"))"#), "{out}");
        // the body's last statement doesn't need a `;`
        assert!(out.contains("a () ;"), "{out}");
    }

    #[test]
    fn plain_branches_have_no_scope() {
        // nothing in these takes a position, so they're plain Rust
        for src in [
            "if ON(pressed) { a += 1 }",
            "if x { UPDATE(text = 1); } else if y { INIT(theme(color, c) = c); } else { { f(); } }",
            "match x { 0 => { if ON(p) { a() } } _ => {} }",
        ] {
            let out = expand(src);
            assert!(!out.contains("__slot"), "{src}\n{out}");
        }
        // these do
        for src in [
            "if x { Label..{}; }",
            "if x { a(); } else { let s = STATE(0); }",
            "if x { if y { Label..{}; } }",
            "match x { 0 => { Label..{}; } _ => {} }",
            "if x { f()..{}; }",
            "if x { { __builder.__child::<Label>(); } }",
        ] {
            let out = expand(src);
            assert!(out.contains("__slot"), "{src}\n{out}");
        }
    }

    #[test]
    fn let_state() {
        let out = expand(
            r#"
            let mut count = STATE(0);
            let items: Vec<String> = STATE(Vec::new());
            "#,
        );
        assert!(!out.contains("compile_error"), "{out}");
        assert!(
            out.contains("let mut count = __builder . __state_mut (|| 0) ;"),
            "{out}"
        );
        assert!(
            out.contains("let items : :: echo :: StateMut < Vec < String > > = __builder . __state_mut (|| Vec :: new ()) ;"),
            "{out}"
        );
        // and taking a position, it's never a plain branch
        assert!(expand("if x { let s = STATE(0); }").contains("__slot"));
    }

    #[test]
    fn misplaced_state() {
        let errs = errors("STATE(0);");
        assert!(errs[0].contains("let mut name = STATE"), "{errs:?}");
        let errs = errors("let (a, b) = STATE((0, 1));");
        assert!(errs[0].contains("plain name"), "{errs:?}");
    }

    #[test]
    fn let_node() {
        let out = expand("let mut a: Gd<Label> = Label..{ INIT(text = 1); }; f(&a);");
        assert!(!out.contains("compile_error"), "{out}");
        assert!(
            out.contains("let mut a : Gd < Label > = { let (__scope , __builder) = __builder . __child :: < Label > () ;"),
            "{out}"
        );
        assert!(out.contains("__builder . node () } ;"), "{out}");
    }

    #[test]
    fn calls() {
        let out = expand(
            r#"
            f(1)..{ Label..{}; };
            g()..|n| { h(n) };
            k()..move |a, b: i32| h(a);
            let v = f(2)..|x| x;
            "#,
        );
        assert!(!out.contains("compile_error"), "{out}");
        assert!(
            out.contains("f (__builder . clone () . upcast () , 1 , & mut | __builder , _ |"),
            "{out}"
        );
        assert!(out.contains("& mut | __builder , n | { h (n) ; }"), "{out}");
        assert!(
            out.contains("& mut move | __builder , (a , b) : (_ , i32) |"),
            "{out}"
        );
        assert!(
            out.contains(
                "let v = { let __scope = __builder . __scope () ; f (__builder . clone () . upcast () , 2 , & mut | __builder , x |"
            ),
            "{out}"
        );
        // a single-element body without braces
        let out = expand("card(\"x\")..|n| Label..{};");
        assert!(!out.contains("compile_error"), "{out}");
        assert!(
            out.contains("& mut | __builder , n | { crate :: __echo_deny ! (Label) ;"),
            "{out}"
        );
        // calls take positions, so a branch with one is scoped
        assert!(expand("if c { f()..|n| {}; }").contains("__slot"));
    }

    #[test]
    fn body_calls() {
        let body = proc_macro2::Ident::new("body", proc_macro2::Span::call_site());
        let rw =
            |src: &str| super::tree::rewrite_body_calls(src.parse().unwrap(), &body).to_string();
        assert_eq!(
            rw("body()"),
            "{ let __scope = __builder . __scope () ; body (__builder . clone () . upcast () , ()) ; }"
        );
        assert_eq!(
            rw("body(42)"),
            "{ let __scope = __builder . __scope () ; body (__builder . clone () . upcast () , 42) ; }"
        );
        assert_eq!(
            rw("Label..{ body(a, f(b, c)) }"),
            "Label .. { { let __scope = __builder . __scope () ; body (__builder . clone () . upcast () , (a , f (b , c))) ; } }"
        );
        // not methods, paths or other names
        assert_eq!(
            rw("x.body(1); m::body(2); other(3)"),
            "x . body (1) ; m :: body (2) ; other (3)"
        );
    }

    #[test]
    fn view_closures() {
        let out = expand(
            r#"
            #[tree] let row = |name: &str| { Label..{}; };
            #[tree(in PanelContainer)] #[allow(unused)] let mut card = |t: &str, content: Body<VBoxContainer, i32>| content(1);
            row("a")..{};
            "#,
        );
        assert!(!out.contains("compile_error"), "{out}");
        assert!(
            out.contains("let row = | __builder : :: echo :: Builder < :: godot :: classes :: Node > , name : & str , __body : :: echo :: Body < '_ > |"),
            "{out}"
        );
        assert!(
            out.contains("# [allow (unused)] # [allow (clippy :: type_complexity)] let mut card = | __builder : :: echo :: Builder < PanelContainer > , t : & str , content : & mut dyn :: core :: ops :: FnMut (:: echo :: Builder < VBoxContainer > , i32) | { let _ : :: core :: marker :: PhantomData < Body < VBoxContainer , i32 > > ; { let __scope = __builder . __scope () ; content (__builder . clone () . upcast () , 1) ; } }"),
            "{out}"
        );
        let errs = errors("#[tree] let x = 5;");
        assert!(errs[0].contains("needs a closure"), "{errs:?}");
        let errs = errors("#[tree(Node())] let x = || {};");
        assert!(errs[0].contains("expected `#[tree]`"), "{errs:?}");
    }

    #[test]
    fn tail_values() {
        // a function body's last statement without `;` is its value
        let block: Block = syn::parse_str("Label..{}; let x = f(); x + 1").unwrap();
        let out = super::tree::gen_body(block).to_string();
        assert!(out.ends_with("let x = f () ; x + 1"), "{out}");
        // elsewhere it's ended
        assert!(expand("if c { f() }").contains("f () ;"));
    }

    #[test]
    fn statements_ending_in_blocks() {
        // they end at their block, so a node right after isn't taken as part of them
        let out =
            expand("while a { b(); } Label..{}; loop { break } 'x: { c() } m! { d } Label..{}");
        assert!(!out.contains("compile_error"), "{out}");
        assert_eq!(out.matches("__child :: < Label >").count(), 2, "{out}");
        assert!(out.starts_with("while a { b () ; }"), "{out}");
    }

    #[test]
    fn nested_items() {
        let out = expand(
            r#"
            fn bar() {}
            Button..{};
            #[derive(Debug)]
            struct S { a: i32 }
            enum E { A }
            impl S { fn f(&self) -> i32 { self.a } }
            use std::fmt;
            const C: i32 = 1;
            static X: i32 = 2;
            type T = i32;
            macro_rules! m { () => {} }
            mod inner { pub fn g() {} }
            trait Tr {}
            unsafe fn u() {}
            Label..{}
            "#,
        );
        assert!(!out.contains("compile_error"), "{out}");
        assert!(out.starts_with("fn bar () { }"), "{out}");
        assert_eq!(out.matches("__child ::").count(), 2, "{out}");
        assert!(
            out.contains("impl S { fn f (& self) -> i32 { self . a } }"),
            "{out}"
        );
        // items don't take positions
        assert!(!expand("if c { fn f() {} struct S; }").contains("__slot"));
        // blocks and closures starting like items still aren't items
        let out = expand("unsafe { a() } Label..{}; async move { b() };");
        assert!(!out.contains("compile_error"), "{out}");
    }

    #[test]
    fn body_calls_once() {
        let body = proc_macro2::Ident::new("body", proc_macro2::Span::call_site());
        let rw =
            |src: &str| super::tree::rewrite_body_calls(src.parse().unwrap(), &body).to_string();
        // a nested function's `body` is its own (its `#[tree]` rewrites it)
        assert_eq!(
            rw("fn inner(body: Body) { body() } body()"),
            "fn inner (body : Body) { body () } { let __scope = __builder . __scope () ; body (__builder . clone () . upcast () , ()) ; }"
        );
        // rewriting twice changes nothing (a closure's own pass after the function's)
        let once = rw("body(1)");
        assert_eq!(rw(&once), once);
    }

    #[test]
    fn on_in_bool_contexts() {
        let out = expand(
            r#"
            if ON(a) && let Some(v) = f() { g(v) }
            if let Some(args) = ON(b) && c { g(args) }
            let both = ON(a) || !(ON(b));
            match x { _ if ON(a) => {} _ => {} }
            h(ON(a));
            let n = ON(a).map(|a| a.len());
            println!("{:?}", ON(a));
            "#,
        );
        assert!(!out.contains("ON ("), "{out}");
        let sig = |name: &str| format!("__builder . __signal (\"{name}\")");
        let fired = |name: &str| format!("{} . is_some ()", sig(name));
        assert!(
            out.contains(&format!("if {} && let Some (v) = f ()", fired("a"))),
            "{out}"
        );
        assert!(
            out.contains(&format!("Some (args) = {} && c", sig("b"))),
            "{out}"
        );
        assert!(
            out.contains(&format!(
                "let both = {} || ! ({}) ;",
                fired("a"),
                fired("b")
            )),
            "{out}"
        );
        assert!(out.contains(&format!("_ if {} =>", fired("a"))), "{out}");
        assert!(out.contains(&format!("h ({}) ;", sig("a"))), "{out}");
        assert!(
            out.contains(&format!("let n = {} . map", sig("a"))),
            "{out}"
        );
        // inside macros, still rewritten (as the arguments)
        assert!(
            out.contains(&format!("println ! (\"{{:?}}\" , {})", sig("a"))),
            "{out}"
        );
    }

    #[test]
    fn removed_forms_explain_themselves() {
        let errs = errors("BODY(1);");
        assert!(errs[0].contains("`BODY(..)` is gone"), "{errs:?}");
        let errs = errors("f()..{ let (a,) = ARGS; g(a) };");
        assert!(errs[0].contains("`let (..) = ARGS;` is gone"), "{errs:?}");
        let errs = errors("#[tree(VBoxContainer(i32))] let c = || {};");
        assert!(errs[0].contains("body: Body<BodyNode, Args>"), "{errs:?}");
        let errs = errors("ON(pressed = |_| a());");
        assert!(
            errs[0].contains("`ON(signal = handler)` is gone"),
            "{errs:?}"
        );
        let errs = errors("STATE(*x = || 1);");
        assert!(
            errs[0].contains("`STATE(name = || init)` is gone"),
            "{errs:?}"
        );
    }

    #[test]
    fn body_types_written_out() {
        let expand = |ty: &str| {
            super::tree::expand_body_type(&syn::parse_str(ty).unwrap())
                .map(|t| quote::quote!(#t).to_string())
        };
        let fnmut = |lt: &str, n: &str, a: &str| {
            format!("& {lt}mut dyn :: core :: ops :: FnMut (:: echo :: Builder < {n} > , {a})")
        };
        assert_eq!(
            expand("Body"),
            Some(fnmut("", ":: godot :: classes :: Node", "()"))
        );
        assert_eq!(expand("Body<Label>"), Some(fnmut("", "Label", "()")));
        assert_eq!(
            expand("echo::Body<Label, &str>"),
            Some(fnmut("", "Label", "& str"))
        );
        assert_eq!(
            expand("Body<'a, Label, (&str, i32)>"),
            Some(fnmut("'a ", "Label", "(& str , i32)"))
        );
        assert_eq!(expand("Vec<i32>"), None);
    }

    #[test]
    fn raw_identifier_props() {
        let out = expand("INIT(r#type = 1);");
        assert!(out.contains(r#"__set_prop ("type" , 1)"#), "{out}");
    }

    #[test]
    fn blocks_are_scopes() {
        // a block is a block, with elements in it
        let out = expand("{ let z = 1; f(z) } { Label..{ INIT(text = 2); }; }");
        assert!(out.starts_with("{ let z = 1 ; f (z) ; }"), "{out}");
        assert!(out.contains("__child :: < Label >"), "{out}");
        // and it never needs a scope of its own
        assert!(!expand("if x { { a(); } }").contains("__slot"));
    }
}

#[cfg(test)]
mod timing {
    use std::time::Instant;

    use proc_macro2::TokenStream;

    use super::tree::{Block, gen_block};

    /// `cargo test -p echo_macro --release -- --ignored --nocapture timing`
    #[test]
    #[ignore]
    fn timing() {
        let src = std::process::Command::new("python3")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../echo_tests/gen_big.py"
            ))
            .output()
            .unwrap()
            .stdout;
        let src = String::from_utf8(src).unwrap();
        // bodies of the four big functions
        let bodies: Vec<String> = src
            .split("pub fn big_")
            .skip(1)
            .map(|f| {
                let open = f.find('{').unwrap();
                let end = f.find("\n}\n").unwrap();
                f[open + 1..end].to_string()
            })
            .collect();
        let toks: Vec<TokenStream> = bodies.iter().map(|b| b.parse().unwrap()).collect();
        let n = 50;
        let time = |name: &str, f: &dyn Fn(&TokenStream)| {
            let t = Instant::now();
            for _ in 0..n {
                f(&toks[1]);
            }
            println!(
                "{name:<40} {:>8.2} ms",
                t.elapsed().as_secs_f64() * 1000.0 / n as f64
            );
        };
        println!("one ~800 line tree function:");
        time("walk tokens only", &|t| {
            fn walk(t: TokenStream) -> usize {
                t.into_iter()
                    .map(|tt| match tt {
                        proc_macro2::TokenTree::Group(g) => 1 + walk(g.stream()),
                        _ => 1,
                    })
                    .sum()
            }
            std::hint::black_box(walk(t.clone()));
        });
        time("parse DSL (syn)", &|t| {
            std::hint::black_box(syn::parse2::<Block>(t.clone()).unwrap());
        });
        time("parse DSL + generate", &|t| {
            std::hint::black_box(gen_block(syn::parse2::<Block>(t.clone()).unwrap()));
        });
        time("parse body as plain Rust (syn::Block)", &|t| {
            let braced: TokenStream = quote::quote!({ #t });
            std::hint::black_box(syn::parse2::<syn::Block>(braced).unwrap());
        });
    }
}
