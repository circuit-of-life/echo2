//! Exports echo's (Godot) examples and tests, then reads the result back through the types.

use std::{fs, path::PathBuf, process::Command};

use echo_export::schema::*;

fn export() -> PathBuf {
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("export");
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
    let status = Command::new(env!("CARGO_BIN_EXE_echo_export"))
        .arg(format!("{root}/echo_examples/src"))
        .arg(format!("{root}/echo_tests/src"))
        .arg("--out")
        .arg(&out)
        .status()
        .unwrap();
    assert!(status.success());
    out
}

/// Every code reference in an element (not in what's nested in it).
fn refs(e: &Element) -> Vec<&CodeRef> {
    let mut out = Vec::new();
    match e {
        Element::LetNode { pat, .. } => out.push(pat),
        Element::Init { props, .. } | Element::Update { props, .. } => {
            for p in props {
                out.extend(p.args.iter().flatten());
                out.push(&p.value);
            }
        }
        Element::Code { args, .. } => out.extend(args.iter().flatten()),
        Element::Call(c) | Element::LetCall(c) => {
            out.extend(&c.pat);
            out.extend(&c.args);
            out.extend(c.body_params.iter().flat_map(|p| &p.pats));
        }
        Element::TreeClosure { name, params, .. } => {
            out.push(name);
            out.extend(params);
        }
        Element::If { branches, .. } => out.extend(branches.iter().map(|b| &b.cond)),
        Element::Match { value, arms, .. } => {
            out.push(value);
            for a in arms {
                out.push(&a.pat);
                out.extend(&a.guard);
            }
        }
        Element::For { pat, iter, key, .. } => out.extend([pat, iter].into_iter().chain(key)),
        Element::State { r#type, init, .. } => out.extend(r#type.iter().chain([init])),
        Element::Memo { value, .. } => out.push(value),
        Element::Node { .. } | Element::Block { .. } | Element::Error { .. } => {}
    }
    out
}

#[test]
fn round_trips_and_resolves() {
    let dir = export();
    let export = Export::load(&dir).unwrap();

    // the types hold everything: writing them back gives the same JSON
    for (file, typed) in [
        ("tree.json", serde_json::to_value(&export.tree).unwrap()),
        ("code.json", serde_json::to_value(&export.code).unwrap()),
    ] {
        let raw: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join(file)).unwrap()).unwrap();
        assert!(raw == typed, "{file} doesn't round-trip");
    }

    // every reference resolves, and its signals are the code's
    let (mut elements, mut errors, mut body_calls) = (0, 0, 0);
    for f in &export.tree.functions {
        for r in f.params.iter().chain(&f.returns) {
            export.code(r);
        }
        walk(&f.body, &mut |e| {
            elements += 1;
            match e {
                Element::Error { .. } => errors += 1,
                Element::Code {
                    code,
                    signals,
                    body_call,
                    ..
                } => {
                    assert_eq!(&export.code(*code).signals, signals);
                    body_calls += *body_call as usize;
                }
                _ => {}
            }
            for r in refs(e) {
                assert_eq!(export.code(r).signals, r.signals);
            }
        });
    }
    assert!(elements > 500, "{elements}");
    assert_eq!(errors, 0);
    assert!(body_calls > 0);

    // a known function
    let card = export
        .tree
        .functions
        .iter()
        .find(|f| f.name == "card")
        .unwrap();
    let body = card.body_param.as_ref().unwrap();
    assert_eq!(body.node.as_deref(), Some("VBoxContainer"));
    let mut nodes = Vec::new();
    walk(&card.body, &mut |e| {
        if let Element::Node { node, .. } = e {
            nodes.push(node.as_str());
        }
    });
    assert!(nodes.contains(&"VBoxContainer"), "{nodes:?}");
}

#[test]
fn code_refs_in_both_forms() {
    let plain: CodeRef = serde_json::from_str(r#""c7""#).unwrap();
    assert_eq!(
        plain,
        CodeRef {
            key: CodeKey(7),
            signals: vec![]
        }
    );
    let with: CodeRef = serde_json::from_str(r#"{"code":"c8","signals":["pressed"]}"#).unwrap();
    assert_eq!(with.signals, ["pressed"]);
    assert_eq!(serde_json::to_string(&plain).unwrap(), r#""c7""#);
    assert_eq!(
        serde_json::to_string(&with).unwrap(),
        r#"{"code":"c8","signals":["pressed"]}"#
    );
    assert!(serde_json::from_str::<CodeKey>(r#""x7""#).is_err());
}
