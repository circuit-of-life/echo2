use std::{
    cell::Cell,
    panic::{AssertUnwindSafe, catch_unwind},
};

use echo::{Body, tree};
use godot::{
    classes::{
        Button, Control, HBoxContainer, HSlider, Label, LineEdit, PanelContainer, VBoxContainer,
    },
    prelude::*,
};

use crate::util::*;

type TestFn = fn(&Gd<Node>) -> Result<(), String>;

pub fn run_all(parent: &Gd<Node>, filter: Option<&str>) -> bool {
    let tests: &[(&str, TestFn)] = &[
        ("basic_reuse", basic_reuse),
        ("if_toggle_keeps_siblings", if_toggle_keeps_siblings),
        ("for_keys_preserve_identity", for_keys_preserve_identity),
        (
            "for_length_change_keeps_following_siblings",
            for_length_change_keeps_following_siblings,
        ),
        ("for_duplicate_keys", for_duplicate_keys),
        ("match_arms", match_arms),
        ("memo_skips_and_reruns", memo_skips_and_reruns),
        (
            "memo_index_after_preceding_change",
            memo_index_after_preceding_change,
        ),
        ("nested_memo_retention", nested_memo_retention),
        ("state_persists_and_resets", state_persists_and_resets),
        ("call_body_args", call_body_args),
        ("removed_nodes_are_freed", removed_nodes_are_freed),
        ("signal_basic", signal_basic),
        ("signal_reconnects_after_skip", signal_reconnects_after_skip),
        ("signal_inside_if", signal_inside_if),
        ("signal_inside_memo", signal_inside_memo),
        ("signal_during_run_is_ignored", signal_during_run_is_ignored),
        (
            "memo_skip_with_removed_sibling_before",
            memo_skip_with_removed_sibling_before,
        ),
        ("nested_memo_slots", nested_memo_slots),
        ("state_inside_skipped_memo", state_inside_skipped_memo),
        (
            "pending_event_survives_skipped_memo",
            pending_event_survives_skipped_memo,
        ),
        ("signal_on_root", signal_on_root),
        (
            "externally_freed_node_is_recreated",
            externally_freed_node_is_recreated,
        ),
        ("panic_mid_run_recovers", panic_mid_run_recovers),
        ("blocks_are_scopes", blocks_are_scopes),
        ("let_and_statements", let_and_statements),
        ("match_guards", match_guards),
        ("path_node_types_and_methods", path_node_types_and_methods),
        ("theme_overrides", theme_overrides),
        ("init_inside_if", init_inside_if),
        (
            "external_changes_are_corrected",
            external_changes_are_corrected,
        ),
        ("fuzz_against_model", fuzz_against_model),
        (
            "freed_node_inside_skipped_memo",
            freed_node_inside_skipped_memo,
        ),
        ("on_handlers_move_values", on_handlers_move_values),
        ("on_expressions", on_expressions),
        (
            "on_expression_break_out_of_loop",
            on_expression_break_out_of_loop,
        ),
        (
            "controlled_line_edit_keeps_caret",
            controlled_line_edit_keeps_caret,
        ),
        ("let_state", let_state),
        ("let_state_kept_past_run", let_state_kept_past_run),
        ("let_node", let_node),
        ("return_values", return_values),
        (
            "multi_arg_bodies_and_usable_in",
            multi_arg_bodies_and_usable_in,
        ),
        ("view_closures", view_closures),
        ("plain_for_loops", plain_for_loops),
        ("nested_items", nested_items),
        ("on_in_conditions", on_in_conditions),
        ("bodies_borrow_locals", bodies_borrow_locals),
        ("early_return_in_call", early_return_in_call),
        ("early_return_in_body", early_return_in_body),
    ];
    let mut failed = 0;
    let mut ran = 0;
    for (name, test) in tests {
        if filter.is_some_and(|f| !name.contains(f)) {
            continue;
        }
        ran += 1;
        match catch_unwind(AssertUnwindSafe(|| test(parent))) {
            Ok(Ok(())) => println!("[PASS] {name}"),
            Ok(Err(e)) => {
                failed += 1;
                println!("[FAIL] {name}\n       {e}");
            }
            Err(p) => {
                failed += 1;
                let msg = p
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();
                println!("[PANIC] {name}\n       {msg}");
            }
        }
    }
    println!("\n{} passed, {} failed", ran - failed, failed);
    failed == 0
}

// ---------------------------------------------------------------------------------------------

struct Basic {
    text: String,
}
#[tree]
fn basic_view(s: &Basic) {
    VBoxContainer..{
        Label..{
            UPDATE(text = s.text.clone());
        };
        Button..{
            INIT(text = "btn");
        };
    };
}
fn basic_reuse(parent: &Gd<Node>) -> Result<(), String> {
    let mut h = Harness::new(parent, Basic { text: "a".into() }, |b, s| {
        basic_view(b, s, &mut |_, _| {})
    });
    h.run();
    check_eq!(descs(&h.root), strs(&["VBoxContainer"]));
    let vbox = kid(&h.root, 0);
    check_eq!(descs(&vbox), strs(&["Label:a", "Button:btn"]));
    let before = ids(&vbox);

    h.state.text = "b".into();
    h.run();
    check_eq!(id(&kid(&h.root, 0)), id(&vbox));
    check_eq!(ids(&vbox), before);
    check_eq!(descs(&vbox), strs(&["Label:b", "Button:btn"]));
    Ok(())
}

// ---------------------------------------------------------------------------------------------

struct IfState {
    show: bool,
}
#[tree]
fn if_view(s: &IfState) {
    if s.show {
        Label..{
            INIT(text = "maybe");
        };
    }
    Label..{
        INIT(text = "after");
    };
}
fn if_toggle_keeps_siblings(parent: &Gd<Node>) -> Result<(), String> {
    let mut h = Harness::new(parent, IfState { show: false }, |b, s| {
        if_view(b, s, &mut |_, _| {})
    });
    h.run();
    check_eq!(descs(&h.root), strs(&["Label:after"]));
    let after = id(&kid(&h.root, 0));

    h.state.show = true;
    h.run();
    check_eq!(descs(&h.root), strs(&["Label:maybe", "Label:after"]));
    check_eq!(id(&kid(&h.root, 1)), after);

    h.state.show = false;
    h.run();
    check_eq!(descs(&h.root), strs(&["Label:after"]));
    check_eq!(id(&kid(&h.root, 0)), after);
    Ok(())
}

// ---------------------------------------------------------------------------------------------

struct ForState {
    items: Vec<i32>,
}
#[tree]
fn for_view(s: &ForState) {
    for i in s.items.iter().copied() {
        KEY(i);
        Label..{
            INIT(text = i.to_string());
        };
    }
    Label..{
        INIT(text = "tail");
    };
}
fn for_keys_preserve_identity(parent: &Gd<Node>) -> Result<(), String> {
    let mut h = Harness::new(
        parent,
        ForState {
            items: vec![1, 2, 3],
        },
        |b, s| for_view(b, s, &mut |_, _| {}),
    );
    h.run();
    check_eq!(
        descs(&h.root),
        strs(&["Label:1", "Label:2", "Label:3", "Label:tail"])
    );
    let old = ids(&h.root);

    h.state.items = vec![2, 3];
    h.run();
    check_eq!(descs(&h.root), strs(&["Label:2", "Label:3", "Label:tail"]));
    check_eq!(ids(&h.root), vec![old[1], old[2], old[3]]);

    h.state.items = vec![3, 2, 5];
    h.run();
    check_eq!(
        descs(&h.root),
        strs(&["Label:3", "Label:2", "Label:5", "Label:tail"])
    );
    let now = ids(&h.root);
    check_eq!(now[0], old[2]);
    check_eq!(now[1], old[1]);
    check_eq!(now[3], old[3]);
    Ok(())
}
fn for_length_change_keeps_following_siblings(parent: &Gd<Node>) -> Result<(), String> {
    let mut h = Harness::new(parent, ForState { items: vec![] }, |b, s| {
        for_view(b, s, &mut |_, _| {})
    });
    h.run();
    let tail = id(&kid(&h.root, 0));
    h.state.items = vec![7, 8];
    h.run();
    check_eq!(descs(&h.root), strs(&["Label:7", "Label:8", "Label:tail"]));
    check_eq!(id(&kid(&h.root, 2)), tail);
    Ok(())
}
fn for_duplicate_keys(parent: &Gd<Node>) -> Result<(), String> {
    let mut h = Harness::new(
        parent,
        ForState {
            items: vec![4, 4, 5],
        },
        |b, s| for_view(b, s, &mut |_, _| {}),
    );
    h.run();
    // Duplicate keys must not collapse into a single node.
    check_eq!(
        descs(&h.root),
        strs(&["Label:4", "Label:4", "Label:5", "Label:tail"])
    );
    let old = ids(&h.root);
    h.run();
    check_eq!(ids(&h.root), old);
    Ok(())
}

// ---------------------------------------------------------------------------------------------

struct MatchState {
    mode: u8,
}
#[tree]
fn match_view(s: &MatchState) {
    match s.mode {
        0 => {
            Label..{
                INIT(text = "zero");
            };
        }
        _ => {
            Button..{
                INIT(text = "other");
            };
        }
    }
    Label..{
        INIT(text = "tail");
    };
}
fn match_arms(parent: &Gd<Node>) -> Result<(), String> {
    let mut h = Harness::new(parent, MatchState { mode: 0 }, |b, s| {
        match_view(b, s, &mut |_, _| {})
    });
    h.run();
    check_eq!(descs(&h.root), strs(&["Label:zero", "Label:tail"]));
    let tail = id(&kid(&h.root, 1));
    h.state.mode = 1;
    h.run();
    check_eq!(descs(&h.root), strs(&["Button:other", "Label:tail"]));
    check_eq!(id(&kid(&h.root, 1)), tail);
    h.state.mode = 0;
    h.run();
    check_eq!(descs(&h.root), strs(&["Label:zero", "Label:tail"]));
    check_eq!(id(&kid(&h.root, 1)), tail);
    Ok(())
}

// ---------------------------------------------------------------------------------------------

struct MemoState {
    m: i32,
    other: i32,
    runs: Cell<u32>,
}
#[tree]
fn memo_view(s: &MemoState) {
    MEMO(s.m) >> {
        {
            s.runs.set(s.runs.get() + 1);
        }
        Label..{
            UPDATE(text = format!("{} {}", s.m, s.other));
        };
    };
}
fn memo_skips_and_reruns(parent: &Gd<Node>) -> Result<(), String> {
    let state = MemoState {
        m: 0,
        other: 0,
        runs: Cell::new(0),
    };
    let mut h = Harness::new(parent, state, |b, s| memo_view(b, s, &mut |_, _| {}));
    h.run();
    check_eq!(h.state.runs.get(), 1);
    let label = id(&kid(&h.root, 0));

    h.state.other = 5;
    h.run();
    h.run();
    check_eq!(h.state.runs.get(), 1);
    check_eq!(descs(&h.root), strs(&["Label:0 0"]));
    check_eq!(id(&kid(&h.root, 0)), label);

    h.state.m = 1;
    h.run();
    check_eq!(h.state.runs.get(), 2);
    check_eq!(descs(&h.root), strs(&["Label:1 5"]));
    check_eq!(id(&kid(&h.root, 0)), label);

    h.run_forced();
    check_eq!(h.state.runs.get(), 3);
    Ok(())
}

struct MemoIdxState {
    flag: bool,
    m: i32,
}
#[tree]
fn memo_idx_view(s: &MemoIdxState) {
    if s.flag {
        Label..{
            INIT(text = "X");
        };
    }
    MEMO(s.m) >> {
        Label..{
            INIT(text = "A");
        };
        Label..{
            INIT(text = "B");
        };
    };
    Label..{
        INIT(text = "C");
    };
}
fn memo_index_after_preceding_change(parent: &Gd<Node>) -> Result<(), String> {
    let mut h = Harness::new(parent, MemoIdxState { flag: false, m: 0 }, |b, s| {
        memo_idx_view(b, s, &mut |_, _| {})
    });
    h.run();
    check_eq!(descs(&h.root), strs(&["Label:A", "Label:B", "Label:C"]));
    h.state.flag = true;
    h.run();
    check_eq!(
        descs(&h.root),
        strs(&["Label:X", "Label:A", "Label:B", "Label:C"])
    );
    h.state.flag = false;
    h.run();
    check_eq!(descs(&h.root), strs(&["Label:A", "Label:B", "Label:C"]));
    Ok(())
}

struct NestedMemo {
    a: i32,
    b: i32,
    show: bool,
}
#[tree]
fn nested_memo_view(s: &NestedMemo) {
    MEMO(s.a) >> {
        Label..{
            INIT(text = "outer");
        };
        MEMO(s.b) >> {
            Label..{
                UPDATE(text = format!("inner {}", s.b));
            };
            if s.show {
                Label..{
                    INIT(text = "shown");
                };
            }
        };
    };
}
fn nested_memo_retention(parent: &Gd<Node>) -> Result<(), String> {
    let mut h = Harness::new(
        parent,
        NestedMemo {
            a: 0,
            b: 0,
            show: true,
        },
        |b, s| nested_memo_view(b, s, &mut |_, _| {}),
    );
    h.run();
    let first = ids(&h.root);
    check_eq!(
        descs(&h.root),
        strs(&["Label:outer", "Label:inner 0", "Label:shown"])
    );
    for _ in 0..3 {
        h.run();
    }
    check_eq!(ids(&h.root), first);

    // outer re-runs, inner is skipped: everything must survive
    h.state.a = 1;
    h.run();
    h.run();
    check_eq!(ids(&h.root), first);

    // inner re-runs (outer must too, to reach it), `shown` goes away
    h.state.a = 2;
    h.state.b = 1;
    h.state.show = false;
    h.run();
    check_eq!(descs(&h.root), strs(&["Label:outer", "Label:inner 1"]));
    check_eq!(ids(&h.root), first[..2].to_vec());
    h.run();
    check_eq!(ids(&h.root), first[..2].to_vec());
    Ok(())
}

// ---------------------------------------------------------------------------------------------

struct StateState {
    show: bool,
    bump: bool,
    seen: Cell<i32>,
}
#[tree]
fn state_view(s: &StateState) {
    if s.show {
        Label..{
            let mut count = STATE(0i32);
            if s.bump {
                *count += 1
            }
            s.seen.set(*count);
        };
    }
}
fn state_persists_and_resets(parent: &Gd<Node>) -> Result<(), String> {
    let state = StateState {
        show: true,
        bump: true,
        seen: Cell::new(-1),
    };
    let mut h = Harness::new(parent, state, |b, s| state_view(b, s, &mut |_, _| {}));
    h.run();
    h.run();
    h.run();
    check_eq!(h.state.seen.get(), 3);
    h.state.show = false;
    h.run();
    h.state.show = true;
    h.state.bump = false;
    h.run();
    check_eq!(h.state.seen.get(), 0);
    Ok(())
}

// ---------------------------------------------------------------------------------------------

#[tree]
fn titled(title: &str, body: Body<VBoxContainer, i32>) {
    VBoxContainer..{
        Label..{
            INIT(text = title.to_string());
        };
        body(42)
    };
}
struct CallState {
    x: i32,
}
#[tree]
fn call_view(s: &CallState) {
    titled("hi")..|n| {
        Label..{
            UPDATE(text = format!("{n}-{}", s.x));
        };
    };
    Label..{
        INIT(text = "tail");
    };
}
fn call_body_args(parent: &Gd<Node>) -> Result<(), String> {
    let mut h = Harness::new(parent, CallState { x: 0 }, |b, s| {
        call_view(b, s, &mut |_, _| {})
    });
    h.run();
    check_eq!(descs(&h.root), strs(&["VBoxContainer", "Label:tail"]));
    let vbox = kid(&h.root, 0);
    check_eq!(descs(&vbox), strs(&["Label:hi", "Label:42-0"]));
    let before = ids(&vbox);
    h.state.x = 1;
    h.run();
    check_eq!(descs(&vbox), strs(&["Label:hi", "Label:42-1"]));
    check_eq!(ids(&vbox), before);
    Ok(())
}

// ---------------------------------------------------------------------------------------------

fn removed_nodes_are_freed(parent: &Gd<Node>) -> Result<(), String> {
    let mut h = Harness::new(parent, ForState { items: vec![1, 2] }, |b, s| {
        for_view(b, s, &mut |_, _| {})
    });
    h.run();
    let old = kids(&h.root);
    h.state.items = vec![];
    h.run();
    check_eq!(descs(&h.root), strs(&["Label:tail"]));
    check!(old[0].is_queued_for_deletion());
    check!(old[1].is_queued_for_deletion());
    check!(!old[2].is_queued_for_deletion());
    // bringing them back creates fresh nodes
    h.state.items = vec![1, 2];
    h.run();
    check_eq!(descs(&h.root), strs(&["Label:1", "Label:2", "Label:tail"]));
    check!(kids(&h.root)[0] != old[0]);
    Ok(())
}

// ---------------------------------------------------------------------------------------------

struct SigState {
    enabled: bool,
    presses: Cell<u32>,
}
fn sig_state(enabled: bool) -> SigState {
    SigState {
        enabled,
        presses: Cell::new(0),
    }
}
fn press(h: &mut Harness<SigState>) {
    let mut button = kid(&h.root, 0);
    button.emit_signal("pressed", &[]);
}

#[tree]
fn sig_basic_view(s: &SigState) {
    Button..{
        if ON(pressed) {
            s.presses.set(s.presses.get() + 1)
        }
    };
}
fn signal_basic(parent: &Gd<Node>) -> Result<(), String> {
    let mut h = Harness::new(parent, sig_state(true), |b, s| {
        sig_basic_view(b, s, &mut |_, _| {})
    });
    h.run();
    press(&mut h);
    check!(h.pump());
    check_eq!(h.state.presses.get(), 1);
    // the event is consumed: later runs don't replay it
    h.run_forced();
    check_eq!(h.state.presses.get(), 1);
    press(&mut h);
    check!(h.pump());
    check_eq!(h.state.presses.get(), 2);
    Ok(())
}

#[tree]
fn sig_toggle_view(s: &SigState) {
    Button..{
        {
            // what `if s.enabled { ON(..) }` lowers to, written by hand so this test
            // doesn't depend on `signal_inside_if` working
            if s.enabled && __builder.__signal("pressed").is_some() {
                s.presses.set(s.presses.get() + 1);
            }
        }
    };
}
fn signal_reconnects_after_skip(parent: &Gd<Node>) -> Result<(), String> {
    let mut h = Harness::new(parent, sig_state(true), |b, s| {
        sig_toggle_view(b, s, &mut |_, _| {})
    });
    h.run();
    h.state.enabled = false;
    h.run();
    press(&mut h);
    check!(!h.pump());
    h.state.enabled = true;
    h.run();
    press(&mut h);
    check!(h.pump());
    check_eq!(h.state.presses.get(), 1);
    Ok(())
}

#[tree]
fn sig_if_view(s: &SigState) {
    Button..{
        if s.enabled {
            if ON(pressed) {
                s.presses.set(s.presses.get() + 1)
            }
        }
    };
}
fn signal_inside_if(parent: &Gd<Node>) -> Result<(), String> {
    let mut h = Harness::new(parent, sig_state(true), |b, s| {
        sig_if_view(b, s, &mut |_, _| {})
    });
    h.run();
    press(&mut h);
    check!(h.pump());
    check_eq!(h.state.presses.get(), 1);
    h.state.enabled = false;
    h.run();
    press(&mut h);
    check!(!h.pump());
    check_eq!(h.state.presses.get(), 1);
    Ok(())
}

#[tree]
fn sig_memo_view(s: &SigState) {
    Button..{
        MEMO(()) >> {
            if ON(pressed) {
                s.presses.set(s.presses.get() + 1)
            }
        };
    };
}
fn signal_inside_memo(parent: &Gd<Node>) -> Result<(), String> {
    let mut h = Harness::new(parent, sig_state(true), |b, s| {
        sig_memo_view(b, s, &mut |_, _| {})
    });
    h.run();
    h.run();
    press(&mut h);
    check!(h.pump());
    check_eq!(h.state.presses.get(), 1);
    Ok(())
}

struct SliderState {
    value: f64,
    changes: Cell<u32>,
}
#[tree]
fn slider_view(s: &SliderState) {
    HSlider..{
        INIT(max_value = 100.0);
        UPDATE(value = s.value);
        if ON(value_changed) {
            s.changes.set(s.changes.get() + 1)
        }
    };
}
fn signal_during_run_is_ignored(parent: &Gd<Node>) -> Result<(), String> {
    let state = SliderState {
        value: 1.0,
        changes: Cell::new(0),
    };
    let mut h = Harness::new(parent, state, |b, s| slider_view(b, s, &mut |_, _| {}));
    h.run();
    h.state.value = 50.0;
    // `UPDATE(value = ..)` makes Godot emit `value_changed` synchronously. That's echo's own
    // write, not user input, so it must not schedule a rerun (or re-enter the app).
    h.run();
    check!(!h.pump());
    check_eq!(h.state.changes.get(), 0);
    // a real change from outside still gets through
    let mut slider = kid(&h.root, 0).cast::<HSlider>();
    slider.set_value(70.0);
    check!(h.pump());
    check_eq!(h.state.changes.get(), 1);
    Ok(())
}

// ---------------------------------------------------------------------------------------------

fn memo_skip_with_removed_sibling_before(parent: &Gd<Node>) -> Result<(), String> {
    // X is removed in the same run that skips the memo, so X is still a (queued) child while
    // A and B have to be placed.
    let mut h = Harness::new(parent, MemoIdxState { flag: true, m: 0 }, |b, s| {
        memo_idx_view(b, s, &mut |_, _| {})
    });
    h.run();
    check_eq!(
        descs(&h.root),
        strs(&["Label:X", "Label:A", "Label:B", "Label:C"])
    );
    h.state.flag = false;
    h.run();
    check_eq!(descs(&h.root), strs(&["Label:A", "Label:B", "Label:C"]));
    // the queued node must have been pushed behind the live ones
    let all: Vec<String> = h
        .root
        .get_children()
        .iter_shared()
        .map(|c| c.get("text").to::<String>())
        .collect();
    check_eq!(all, strs(&["A", "B", "C", "X"]));
    Ok(())
}

struct NestedSlots {
    flag: bool,
    a: i32,
    b: i32,
}
#[tree]
fn nested_slots_view(s: &NestedSlots) {
    if s.flag {
        Label..{
            INIT(text = "X");
        };
    }
    MEMO(s.a) >> {
        MEMO(s.b) >> {
            Label..{
                INIT(text = "A");
            };
            Label..{
                INIT(text = "B");
            };
        };
        Label..{
            INIT(text = "C");
        };
    };
    Label..{
        INIT(text = "D");
    };
}
fn nested_memo_slots(parent: &Gd<Node>) -> Result<(), String> {
    let state = NestedSlots {
        flag: false,
        a: 0,
        b: 0,
    };
    let mut h = Harness::new(parent, state, |b, s| {
        nested_slots_view(b, s, &mut |_, _| {})
    });
    h.run();
    // outer re-runs, inner skipped: the inner memo's nodes must still count as the outer's
    h.state.a = 1;
    h.run();
    check_eq!(
        descs(&h.root),
        strs(&["Label:A", "Label:B", "Label:C", "Label:D"])
    );
    let ids0 = ids(&h.root);
    // now both are skipped and everything shifts by one
    h.state.flag = true;
    h.run();
    check_eq!(
        descs(&h.root),
        strs(&["Label:X", "Label:A", "Label:B", "Label:C", "Label:D"])
    );
    check_eq!(ids(&h.root)[1..].to_vec(), ids0);
    h.state.flag = false;
    h.run();
    check_eq!(ids(&h.root), ids0);
    Ok(())
}

struct MemoState2 {
    m: i32,
    bump: bool,
    seen: Cell<i32>,
}
#[tree]
fn state_in_memo_view(s: &MemoState2) {
    MEMO(s.m) >> {
        let mut count = STATE(0);
        if s.bump {
            *count += 1
        }
        {
            s.seen.set(*count);
        }
    };
}
fn state_inside_skipped_memo(parent: &Gd<Node>) -> Result<(), String> {
    let state = MemoState2 {
        m: 0,
        bump: true,
        seen: Cell::new(-1),
    };
    let mut h = Harness::new(parent, state, |b, s| {
        state_in_memo_view(b, s, &mut |_, _| {})
    });
    h.run();
    check_eq!(h.state.seen.get(), 1);
    for _ in 0..3 {
        h.run(); // skipped: state must survive without being touched
    }
    h.state.m = 1;
    h.run();
    check_eq!(h.state.seen.get(), 2);
    Ok(())
}

fn pending_event_survives_skipped_memo(parent: &Gd<Node>) -> Result<(), String> {
    // As if `__echo_rerun` were connected deferred: a normal run happens first, and the memo
    // around the `ON` is skipped in it. The event must still reach the handler afterwards.
    let mut h = Harness::new(parent, sig_state(true), |b, s| {
        sig_memo_view(b, s, &mut |_, _| {})
    });
    h.run();
    let mut button = kid(&h.root, 0);
    button.emit_signal("pressed", &[]);
    h.run();
    check_eq!(h.state.presses.get(), 0);
    check!(h.pump());
    check_eq!(h.state.presses.get(), 1);
    h.run_forced();
    check_eq!(h.state.presses.get(), 1);
    Ok(())
}

#[tree]
fn root_signal_view(s: &SigState) {
    if ON(renamed) {
        s.presses.set(s.presses.get() + 1)
    }
}
fn signal_on_root(parent: &Gd<Node>) -> Result<(), String> {
    let mut h = Harness::new(parent, sig_state(true), |b, s| {
        root_signal_view(b, s, &mut |_, _| {})
    });
    h.run();
    h.root.emit_signal("renamed", &[]);
    check!(h.pump());
    check_eq!(h.state.presses.get(), 1);
    Ok(())
}

fn externally_freed_node_is_recreated(parent: &Gd<Node>) -> Result<(), String> {
    let mut h = Harness::new(parent, Basic { text: "a".into() }, |b, s| {
        basic_view(b, s, &mut |_, _| {})
    });
    h.run();
    kid(&kid(&h.root, 0), 0).free();
    h.run();
    check_eq!(descs(&kid(&h.root, 0)), strs(&["Label:a", "Button:btn"]));
    kid(&h.root, 0).free();
    h.run();
    check_eq!(descs(&h.root), strs(&["VBoxContainer"]));
    check_eq!(descs(&kid(&h.root, 0)), strs(&["Label:a", "Button:btn"]));
    Ok(())
}

struct PanicState {
    panic: bool,
    items: Vec<i32>,
}
#[tree]
fn panic_view(s: &PanicState) {
    for i in s.items.iter().copied() {
        KEY(i);
        VBoxContainer..{
            Label..{
                INIT(text = i.to_string());
            };
            {
                if s.panic && i == 2 {
                    panic!("boom");
                }
            }
        };
    }
}
fn panic_mid_run_recovers(parent: &Gd<Node>) -> Result<(), String> {
    let mut h = Harness::new(
        parent,
        PanicState {
            panic: false,
            items: vec![1, 2, 3],
        },
        |b, s| panic_view(b, s, &mut |_, _| {}),
    );
    h.run();
    let before = ids(&h.root);
    h.state.panic = true;
    h.state.items = vec![1, 2, 3, 4];
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let r = catch_unwind(AssertUnwindSafe(|| h.run()));
    std::panic::set_hook(hook);
    check!(r.is_err());
    h.state.panic = false;
    h.state.items = vec![1, 3];
    h.run();
    check_eq!(kids(&h.root).len(), 2);
    let after = ids(&h.root);
    check_eq!(after[0], before[0]);
    check_eq!(after[1], before[2]);
    check_eq!(descs(&kid(&h.root, 1)), strs(&["Label:3"]));
    Ok(())
}

struct TextState {
    n: i32,
}
#[tree]
fn block_view(s: &TextState) {
    let text = format!("n = {}", s.n);
    {
        // a block scope, with elements in it
        let text = "shadowed";
        Label..{
            UPDATE(text = text);
        };
    }
    Label..{
        UPDATE(text = text.clone());
    };
    let mut i = 0;
    while i < 2 {
        i += 1
    }
    Label..{
        UPDATE(text = i.to_string());
    };
}
fn blocks_are_scopes(parent: &Gd<Node>) -> Result<(), String> {
    let mut h = Harness::new(parent, TextState { n: 3 }, |b, s| {
        block_view(b, s, &mut |_, _| {})
    });
    h.run();
    check_eq!(
        descs(&h.root),
        strs(&["Label:shadowed", "Label:n = 3", "Label:2"])
    );
    let first = ids(&h.root);
    h.state.n = 4;
    h.run();
    check_eq!(
        descs(&h.root),
        strs(&["Label:shadowed", "Label:n = 4", "Label:2"])
    );
    check_eq!(ids(&h.root), first);
    Ok(())
}

#[tree]
fn let_view(s: &TextState) {
    let doubled = s.n * 2;
    let range = 0..doubled;
    Label..{
        UPDATE(text = format!("{doubled} {}", range.len()));
    };
}
fn let_and_statements(parent: &Gd<Node>) -> Result<(), String> {
    let mut h = Harness::new(parent, TextState { n: 2 }, |b, s| {
        let_view(b, s, &mut |_, _| {})
    });
    h.run();
    check_eq!(descs(&h.root), strs(&["Label:4 4"]));
    Ok(())
}

#[tree]
fn guard_view(s: &TextState) {
    match s.n {
        n if n < 0 => {
            Label..{
                INIT(text = "neg");
            };
        }
        0 => {}
        _ => {
            Button..{
                INIT(text = "pos");
            };
        }
    }
}
fn match_guards(parent: &Gd<Node>) -> Result<(), String> {
    let mut h = Harness::new(parent, TextState { n: -1 }, |b, s| {
        guard_view(b, s, &mut |_, _| {})
    });
    h.run();
    check_eq!(descs(&h.root), strs(&["Label:neg"]));
    h.state.n = 0;
    h.run();
    check_eq!(descs(&h.root), Vec::<String>::new());
    h.state.n = 5;
    h.run();
    check_eq!(descs(&h.root), strs(&["Button:pos"]));
    Ok(())
}

struct Widgets {
    prefix: &'static str,
}
impl Widgets {
    #[tree]
    fn titled(&self, n: i32, body: Body<Label>) {
        godot::classes::Label..{
            INIT(text = format!("{}{n}", self.prefix));
            body();
        };
    }
}
#[tree]
fn method_view(w: &Widgets) {
    w.titled(1)..{
        Label..{
            INIT(text = "inner");
        };
    };
}
fn path_node_types_and_methods(parent: &Gd<Node>) -> Result<(), String> {
    let mut h = Harness::new(parent, Widgets { prefix: "w" }, |b, s| {
        method_view(b, s, &mut |_, _| {})
    });
    h.run();
    check_eq!(descs(&h.root), strs(&["Label:w1"]));
    check_eq!(descs(&kid(&h.root, 0)), strs(&["Label:inner"]));
    Ok(())
}

#[tree]
fn theme_view(s: &TextState) {
    Label..{
        INIT(
            theme(color, font_color) = Color::RED,
            theme(constant, outline_size) = 3,
        );
        UPDATE(theme(font_size, font_size) = 10 + s.n);
    };
}
fn theme_overrides(parent: &Gd<Node>) -> Result<(), String> {
    let mut h = Harness::new(parent, TextState { n: 2 }, |b, s| {
        theme_view(b, s, &mut |_, _| {})
    });
    h.run();
    let label = kid(&h.root, 0).cast::<Control>();
    check!(label.has_theme_color_override("font_color"));
    check_eq!(label.get_theme_constant("outline_size"), 3);
    check_eq!(label.get_theme_font_size("font_size"), 12);
    h.state.n = 4;
    h.run();
    check_eq!(label.get_theme_font_size("font_size"), 14);
    Ok(())
}

struct InitIf {
    on: bool,
    runs: Cell<u32>,
}
#[tree]
fn init_if_view(s: &InitIf) {
    Label..{
        if s.on {
            INIT(text = "first");
        }
        {
            if __builder.init() {
                s.runs.set(s.runs.get() + 1);
            }
        }
    };
}
fn init_inside_if(parent: &Gd<Node>) -> Result<(), String> {
    let state = InitIf {
        on: true,
        runs: Cell::new(0),
    };
    let mut h = Harness::new(parent, state, |b, s| init_if_view(b, s, &mut |_, _| {}));
    h.run();
    h.run();
    check_eq!(descs(&h.root), strs(&["Label:first"]));
    check_eq!(h.state.runs.get(), 1);
    Ok(())
}

// ---------------------------------------------------------------------------------------------

fn external_changes_are_corrected(parent: &Gd<Node>) -> Result<(), String> {
    let mut h = Harness::new(
        parent,
        ForState {
            items: vec![1, 2, 3],
        },
        |b, s| for_view(b, s, &mut |_, _| {}),
    );
    let want = strs(&["Label:1", "Label:2", "Label:3", "Label:tail"]);
    h.run();
    h.run();
    let tail = kid(&h.root, 3);
    h.root.move_child(&tail, 0);
    h.run();
    check_eq!(descs(&h.root), want);
    h.run();
    check_eq!(descs(&h.root), want);
    let foreign = Node::new_alloc();
    h.root.add_child(&foreign);
    h.root.move_child(&foreign, 1);
    h.run();
    check_eq!(kids(&h.root).len(), 5);
    check_eq!(kids(&h.root)[4], foreign);
    h.run();
    check_eq!(kids(&h.root)[4], foreign);
    Ok(())
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

struct Fuzz {
    head: bool,
    items: Vec<(u32, bool, u32)>,
    mode: u8,
}
#[tree]
fn fuzz_view(s: &Fuzz) {
    if s.head {
        Label..{
            INIT(text = "head");
        };
    }
    MEMO(()) >> {
        Label..{
            INIT(text = "fixed");
        };
    };
    for (k, extra, m) in s.items.iter().copied() {
        KEY(k);
        MEMO((extra, m)) >> {
            Label..{
                INIT(text = format!("{k}"));
            };
            if extra {
                Label..{
                    INIT(text = format!("{k}+"));
                };
            }
        };
    }
    match s.mode {
        0 => {
            Label..{
                INIT(text = "m0");
            };
        }
        1 => {}
        _ => {
            Button..{
                INIT(text = "m2");
            };
            Label..{
                INIT(text = "m2b");
            };
        }
    }
    Label..{
        INIT(text = "tail");
    };
}
fn fuzz_expected(s: &Fuzz) -> Vec<String> {
    let mut v = vec![];
    if s.head {
        v.push("Label:head".to_string());
    }
    v.push("Label:fixed".to_string());
    for (k, extra, _) in &s.items {
        v.push(format!("Label:{k}"));
        if *extra {
            v.push(format!("Label:{k}+"));
        }
    }
    match s.mode {
        0 => v.push("Label:m0".into()),
        1 => {}
        _ => v.extend(["Button:m2".to_string(), "Label:m2b".to_string()]),
    }
    v.push("Label:tail".into());
    v
}
fn fuzz_against_model(parent: &Gd<Node>) -> Result<(), String> {
    use std::collections::HashMap;
    let state = Fuzz {
        head: false,
        items: (0..6).map(|k| (k, k % 2 == 0, 0)).collect(),
        mode: 0,
    };
    let mut h = Harness::new(parent, state, |b, s| fuzz_view(b, s, &mut |_, _| {}));
    let mut rng = Rng(0x2545F4914F6CDD1D);
    let mut next_key = 100;
    let mut foreign = vec![];
    let mut item_ids: HashMap<u32, i64> = HashMap::new();
    for step in 0..400 {
        let s = &mut h.state;
        match rng.below(12) {
            0 => {
                let at = rng.below(s.items.len() + 1);
                s.items.insert(at, (next_key, rng.below(2) == 0, 0));
                next_key += 1;
            }
            1 if !s.items.is_empty() => {
                let at = rng.below(s.items.len());
                s.items.remove(at);
            }
            2 if s.items.len() > 1 => {
                let (a, b) = (rng.below(s.items.len()), rng.below(s.items.len()));
                s.items.swap(a, b);
            }
            3 if !s.items.is_empty() => {
                let n = rng.below(s.items.len());
                s.items.rotate_left(n);
            }
            4 if !s.items.is_empty() => {
                let i = rng.below(s.items.len());
                s.items[i].1 = !s.items[i].1;
            }
            5 if !s.items.is_empty() => {
                let i = rng.below(s.items.len());
                s.items[i].2 += 1;
            }
            6 => s.head = !s.head,
            7 => s.mode = rng.below(3) as u8,
            8 => {
                // someone else reorders our nodes
                let k = kids(&h.root);
                let (a, b) = (rng.below(k.len()), rng.below(k.len()));
                h.root.move_child(&k[a], b as i32);
            }
            9 if foreign.len() < 3 => {
                // someone else adds a node in the middle
                let n = Node::new_alloc();
                h.root.add_child(&n);
                let at = rng.below(kids(&h.root).len());
                h.root.move_child(&n, at as i32);
                foreign.push(n);
            }
            _ => {}
        }
        h.run();
        let live: Vec<Gd<Node>> = kids(&h.root)
            .into_iter()
            .filter(|c| !foreign.contains(c))
            .collect();
        let got: Vec<String> = descs(&h.root)
            .into_iter()
            .zip(kids(&h.root))
            .filter(|(_, c)| !foreign.contains(c))
            .map(|(d, _)| d)
            .collect();
        let want = fuzz_expected(&h.state);
        if got != want {
            return Err(format!("step {step}: got {got:?}\n       want {want:?}"));
        }
        // echo's nodes come first, foreign ones after
        let all = kids(&h.root);
        check!(all[..live.len()] == live[..]);
        // items keep their nodes for as long as they exist
        let mut now = HashMap::new();
        for c in &live {
            let text = c.get("text").to::<String>();
            if let Ok(k) = text.parse::<u32>() {
                now.insert(k, id(c));
            }
        }
        for (k, i) in &now {
            if item_ids.get(k).is_some_and(|old| old != i) {
                return Err(format!("step {step}: item {k} got a new node"));
            }
        }
        item_ids = now;
    }
    Ok(())
}

fn freed_node_inside_skipped_memo(parent: &Gd<Node>) -> Result<(), String> {
    let mut h = Harness::new(parent, MemoIdxState { flag: false, m: 0 }, |b, s| {
        memo_idx_view(b, s, &mut |_, _| {})
    });
    h.run();
    h.run();
    kid(&h.root, 0).free(); // "A", inside the memo
    h.run(); // memo skipped
    check_eq!(descs(&h.root), strs(&["Label:B", "Label:C"]));
    h.state.flag = true;
    h.run();
    check_eq!(descs(&h.root), strs(&["Label:X", "Label:B", "Label:C"]));
    // re-running the memo brings A back
    h.state.m = 1;
    h.run();
    check_eq!(
        descs(&h.root),
        strs(&["Label:X", "Label:A", "Label:B", "Label:C"])
    );
    Ok(())
}

struct OnceState {
    n: i32,
    log: std::cell::RefCell<Vec<String>>,
    value: Cell<f64>,
}
#[tree]
fn on_once_view(s: &OnceState) {
    Button..{
        let label = format!("clicked {}", s.n);
        // a handler can move values: it's just code
        if ON(pressed) {
            s.log.borrow_mut().push(label)
        }
    };
    HSlider..{
        INIT(max_value = 100.0);
        if let Some(args) = ON(value_changed) {
            s.value.set(args[0].to::<f64>())
        }
    };
}
fn on_handlers_move_values(parent: &Gd<Node>) -> Result<(), String> {
    let state = OnceState {
        n: 3,
        log: Default::default(),
        value: Cell::new(0.0),
    };
    let mut h = Harness::new(parent, state, |b, s| on_once_view(b, s, &mut |_, _| {}));
    h.run();
    kid(&h.root, 0).emit_signal("pressed", &[]);
    check!(h.pump());
    check_eq!(*h.state.log.borrow(), vec!["clicked 3".to_string()]);
    kid(&h.root, 1).cast::<HSlider>().set_value(42.0);
    check!(h.pump());
    check_eq!(h.state.value.get(), 42.0);
    Ok(())
}

#[tree]
fn on_expr_view(s: &OnceState) {
    if let Some(args) = ON(renamed) {
        s.log
            .borrow_mut()
            .push(format!("root renamed, {} args", args.len()))
    }
    Button..{
        if ON(pressed) {
            s.log.borrow_mut().push(format!("pressed {}", s.n))
        }
    };
    HSlider..{
        INIT(max_value = 100.0);
        if let Some(args) = ON(value_changed) {
            s.value.set(args[0].to::<f64>())
        }
    };
}
fn on_expressions(parent: &Gd<Node>) -> Result<(), String> {
    let state = OnceState {
        n: 7,
        log: Default::default(),
        value: Cell::new(0.0),
    };
    let mut h = Harness::new(parent, state, |b, s| on_expr_view(b, s, &mut |_, _| {}));
    h.run();
    h.run();
    check!(!h.pump());
    kid(&h.root, 0).emit_signal("pressed", &[]);
    check!(h.pump());
    check_eq!(*h.state.log.borrow(), vec!["pressed 7".to_string()]);
    kid(&h.root, 1).cast::<HSlider>().set_value(42.0);
    check!(h.pump());
    check_eq!(h.state.value.get(), 42.0);
    h.root.emit_signal("renamed", &[]);
    check!(h.pump());
    check_eq!(h.state.log.borrow()[1], "root renamed, 0 args");
    // events are consumed: nothing runs twice
    h.run_forced();
    check_eq!(h.state.log.borrow().len(), 2);
    Ok(())
}

#[tree]
fn on_break_view(s: &ForState) {
    for i in s.items.iter().copied() {
        KEY(i);
        Button..{
            INIT(text = i.to_string());
            if ON(pressed) {
                // plain Rust control flow, not a closure: leaves the loop
                break;
            }
        };
    }
    Label..{
        INIT(text = "tail");
    };
}
fn on_expression_break_out_of_loop(parent: &Gd<Node>) -> Result<(), String> {
    let mut h = Harness::new(
        parent,
        ForState {
            items: vec![0, 1, 2],
        },
        |b, s| on_break_view(b, s, &mut |_, _| {}),
    );
    h.run();
    let before = ids(&h.root);
    kid(&h.root, 1).emit_signal("pressed", &[]);
    check!(h.pump());
    // the run stopped at item 1, so item 2 wasn't built this time
    check_eq!(
        descs(&h.root),
        strs(&["Button:0", "Button:1", "Label:tail"])
    );
    check_eq!(ids(&h.root), vec![before[0], before[1], before[3]]);
    h.run();
    check_eq!(
        descs(&h.root),
        strs(&["Button:0", "Button:1", "Button:2", "Label:tail"])
    );
    Ok(())
}

struct InputState {
    input: String,
}
// A LineEdit whose text lives in the state, as in the todo example.
#[tree]
fn line_edit_view(s: &mut InputState) {
    LineEdit..{
        // the event first, so the check below compares against the new text
        if let Some(args) = ON(text_changed) {
            s.input = args[0].to()
        }
        if __builder.node().get_text() != s.input.as_str() {
            UPDATE(text = s.input.clone());
        }
    };
}
fn controlled_line_edit_keeps_caret(parent: &Gd<Node>) -> Result<(), String> {
    let mut h = Harness::new(parent, InputState { input: "ac".into() }, |b, s| {
        line_edit_view(b, s, &mut |_, _| {})
    });
    h.run();
    let mut edit = kid(&h.root, 0).cast::<LineEdit>();
    check_eq!(edit.get_text().to_string(), "ac");
    // type "b" in the middle, like a user would
    edit.set_caret_column(1);
    edit.insert_text_at_caret("b");
    if !h.pump() {
        // in case this Godot doesn't emit it for `insert_text_at_caret`
        edit.emit_signal("text_changed", &["abc".to_variant()]);
        check!(h.pump());
    }
    h.run();
    h.run();
    check_eq!(h.state.input, "abc");
    check_eq!(edit.get_text().to_string(), "abc");
    check_eq!(edit.get_caret_column(), 2);
    // changing the state still updates it
    h.state.input.clear();
    h.run();
    check_eq!(edit.get_text().to_string(), "");
    Ok(())
}

struct LetStateState {
    show: bool,
    bump: bool,
}
fn push_twice(items: &mut Vec<u32>, n: u32) {
    items.push(n);
    items.push(n);
}
#[tree]
fn let_state_view(s: &LetStateState) {
    if s.show {
        Label..{
            let mut count = STATE(0);
            let mut items: Vec<u32> = STATE(Vec::new());
            if s.bump {
                *count += 1;
                push_twice(&mut items, *count)
            }
            UPDATE(text = format!("{count} {items:?}"));
        };
    }
}
fn let_state(parent: &Gd<Node>) -> Result<(), String> {
    let state = LetStateState {
        show: true,
        bump: true,
    };
    let mut h = Harness::new(parent, state, |b, s| let_state_view(b, s, &mut |_, _| {}));
    h.run();
    h.run();
    check_eq!(descs(&h.root), strs(&["Label:2 [1, 1, 2, 2]"]));
    h.state.bump = false;
    h.run();
    check_eq!(descs(&h.root), strs(&["Label:2 [1, 1, 2, 2]"]));
    // gone with its branch, fresh when it's back
    h.state.show = false;
    h.run();
    h.state.show = true;
    h.run();
    check_eq!(descs(&h.root), strs(&["Label:0 []"]));
    Ok(())
}

struct KeepState {
    kept: std::cell::RefCell<Option<echo::StateMut<i32>>>,
    keep: bool,
}
#[tree]
fn keep_view(s: &KeepState) {
    let mut n = STATE(10);
    *n += 1;
    if s.keep {
        *s.kept.borrow_mut() = Some(n)
    }
}
fn let_state_kept_past_run(parent: &Gd<Node>) -> Result<(), String> {
    let state = KeepState {
        kept: Default::default(),
        keep: true,
    };
    let mut h = Harness::new(parent, state, |b, s| keep_view(b, s, &mut |_, _| {}));
    h.run();
    h.state.keep = false;
    // the kept guard still has the state: using it again must panic, not alias
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let r = catch_unwind(AssertUnwindSafe(|| h.run()));
    std::panic::set_hook(hook);
    let msg = r
        .err()
        .and_then(|p| {
            (p.downcast_ref::<String>().cloned())
                .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
        })
        .unwrap_or_default();
    check!(msg.contains("still in use"));
    // once it's given back, the state is usable (and kept its value)
    let kept = h.state.kept.borrow_mut().take().unwrap();
    check_eq!(*kept, 11);
    drop(kept);
    h.run();
    h.state.keep = true;
    h.run();
    let value = **h.state.kept.borrow().as_ref().unwrap();
    check_eq!(value, 13);
    Ok(())
}

struct LetNodeState {
    n: i32,
    m: i32,
}
#[tree]
fn let_node_view(s: &LetNodeState) {
    let label = Label..{
        UPDATE(text = format!("n = {}", s.n));
    };
    // siblings can use it
    Button..{
        UPDATE(text = label.get_text());
    };
    let mut edit = LineEdit..{};
    Button..{
        if ON(pressed) {
            edit.set_text("pressed")
        }
    };
    MEMO(s.m) >> {
        let inner = Label..{
            INIT(text = "inner");
        };
        { inner.get_text() }
    };
}
fn let_node(parent: &Gd<Node>) -> Result<(), String> {
    let mut h = Harness::new(parent, LetNodeState { n: 1, m: 0 }, |b, s| {
        let_node_view(b, s, &mut |_, _| {})
    });
    h.run();
    check_eq!(
        descs(&h.root),
        strs(&[
            "Label:n = 1",
            "Button:n = 1",
            "LineEdit:",
            "Button:",
            "Label:inner"
        ])
    );
    let first = ids(&h.root);
    h.state.n = 2;
    h.run();
    check_eq!(descs(&h.root)[1], "Button:n = 2");
    kid(&h.root, 3).emit_signal("pressed", &[]);
    check!(h.pump());
    check_eq!(descs(&h.root)[2], "LineEdit:pressed");
    h.run();
    check_eq!(ids(&h.root), first);
    Ok(())
}

struct RetState {
    count: i32,
    log: Vec<&'static str>,
}
/// Tells its caller whether it was clicked this run.
#[tree]
fn count_button(label: &str, count: &mut i32) -> bool {
    let mut clicked = false;
    Button..{
        INIT(text = label.to_string());
        if ON(pressed) {
            *count += 1;
            clicked = true
        }
    };
    clicked
}
#[tree]
fn return_view(s: &mut RetState) {
    let clicked = count_button("a", &mut s.count)..{};
    if clicked {
        s.log.push("clicked")
    }
}
fn return_values(parent: &Gd<Node>) -> Result<(), String> {
    let state = RetState {
        count: 0,
        log: vec![],
    };
    let mut h = Harness::new(parent, state, |b, s| return_view(b, s, &mut |_, _| {}));
    h.run();
    h.run();
    check_eq!(h.state.log, Vec::<&str>::new());
    kid(&h.root, 0).emit_signal("pressed", &[]);
    check!(h.pump());
    check_eq!(h.state.count, 1);
    check_eq!(h.state.log, vec!["clicked"]);
    h.run();
    check_eq!(h.state.log, vec!["clicked"]);
    Ok(())
}

#[tree]
fn pair(body: Body<VBoxContainer, (i32, String)>) {
    VBoxContainer..{
        body(1, "one".to_string());
        body(2, "two".to_string())
    };
}
#[tree(in PanelContainer)]
fn inside_panel(text: &str) {
    Label..{
        UPDATE(text = text.to_string());
    };
}
#[tree]
fn multi_view(s: &CallState) {
    pair()..|n, text: String| {
        Label..{
            UPDATE(text = format!("{n} {text} {}", s.x));
        };
    };
    PanelContainer..{
        inside_panel("in a panel")..{};
    };
}
fn multi_arg_bodies_and_usable_in(parent: &Gd<Node>) -> Result<(), String> {
    let mut h = Harness::new(parent, CallState { x: 5 }, |b, s| {
        multi_view(b, s, &mut |_, _| {})
    });
    h.run();
    check_eq!(
        descs(&kid(&h.root, 0)),
        strs(&["Label:1 one 5", "Label:2 two 5"])
    );
    check_eq!(descs(&kid(&h.root, 1)), strs(&["Label:in a panel"]));
    Ok(())
}

struct ClosureState {
    a: i32,
    show: bool,
    len: usize,
}
#[tree]
fn closure_view(s: &mut ClosureState) {
    #[tree]
    let row = |name: &str, n: i32| {
        HBoxContainer..{
            Label..{
                UPDATE(text = name.to_string());
            };
            Label..{
                UPDATE(text = n.to_string());
            };
        }
    };
    row("a", s.a)..{};
    if s.show {
        row("maybe", 0)..{};
    }
    row("b", 2)..{};

    // with a body, and a return value
    #[tree]
    let boxed = |title: &str, body: Body<VBoxContainer>| -> usize {
        VBoxContainer..{
            Label..{
                UPDATE(text = title.to_string());
            };
            body()
        };
        title.len()
    };
    let len = boxed("box")..{
        Label..{
            INIT(text = "inside");
        }
    };
    s.len = len;
}
fn view_closures(parent: &Gd<Node>) -> Result<(), String> {
    let state = ClosureState {
        a: 1,
        show: false,
        len: 0,
    };
    let mut h = Harness::new(parent, state, |b, s| closure_view(b, s, &mut |_, _| {}));
    h.run();
    check_eq!(
        descs(&h.root),
        strs(&["HBoxContainer", "HBoxContainer", "VBoxContainer"])
    );
    check_eq!(descs(&kid(&h.root, 0)), strs(&["Label:a", "Label:1"]));
    check_eq!(
        descs(&kid(&h.root, 2)),
        strs(&["Label:box", "Label:inside"])
    );
    check_eq!(h.state.len, 3);
    let before = ids(&h.root);
    // calls in a branch don't disturb what comes after
    h.state.show = true;
    h.state.a = 9;
    h.run();
    check_eq!(descs(&kid(&h.root, 1)), strs(&["Label:maybe", "Label:0"]));
    check_eq!(descs(&kid(&h.root, 0)), strs(&["Label:a", "Label:9"]));
    let now = ids(&h.root);
    check_eq!(vec![now[0], now[2], now[3]], before);
    Ok(())
}

struct PlainForState {
    items: Vec<i32>,
    pressed: Cell<u32>,
}
#[tree]
fn plain_for_view(s: &PlainForState) {
    // plain loops: no KEY needed, `continue`/`break` work as usual
    let mut total = 0;
    for i in s.items.iter() {
        if *i < 0 {
            continue;
        }
        total += i
    }
    Label..{
        UPDATE(text = total.to_string());
    };
    Button..{
        for name in ["pressed", "toggled"] {
            if name == "pressed" && ON(pressed).is_some() {
                s.pressed.set(s.pressed.get() + 1)
            }
        }
    };
    // and a keyed one next to them
    for i in s.items.iter() {
        KEY(*i);
        Label..{
            UPDATE(text = i.to_string());
        };
    }
}
fn plain_for_loops(parent: &Gd<Node>) -> Result<(), String> {
    let state = PlainForState {
        items: vec![1, -5, 2],
        pressed: Cell::new(0),
    };
    let mut h = Harness::new(parent, state, |b, s| plain_for_view(b, s, &mut |_, _| {}));
    h.run();
    check_eq!(
        descs(&h.root),
        strs(&["Label:3", "Button:", "Label:1", "Label:-5", "Label:2"])
    );
    let first = ids(&h.root);
    h.state.items = vec![1, -5, 2, 4];
    h.run();
    check_eq!(descs(&h.root)[0], "Label:7");
    check_eq!(ids(&h.root)[..5].to_vec(), first);
    kid(&h.root, 1).emit_signal("pressed", &[]);
    check!(h.pump());
    check_eq!(h.state.pressed.get(), 1);
    Ok(())
}

#[tree]
fn nested_items_view(s: &CallState, body: Body<VBoxContainer>) {
    const PREFIX: &str = "n";
    struct Pair(i32, i32);
    impl Pair {
        fn sum(&self) -> i32 {
            self.0 + self.1
        }
    }
    fn double(x: i32) -> i32 {
        x * 2
    }
    // a nested tree function whose body parameter has the same name as ours
    #[tree]
    fn framed(title: &str, body: Body<VBoxContainer>) {
        VBoxContainer..{
            Label..{
                UPDATE(text = title.to_string());
            };
            body()
        };
    }

    Label..{
        UPDATE(text = format!("{PREFIX}{}", double(Pair(s.x, 1).sum())));
    };
    framed("inner")..{
        // this `body` is ours
        VBoxContainer..{ body() };
    };
}
fn nested_items(parent: &Gd<Node>) -> Result<(), String> {
    let mut h = Harness::new(parent, CallState { x: 1 }, |b, s| {
        nested_items_view(b, s, &mut |_, _| {})
    });
    h.run();
    check_eq!(descs(&h.root), strs(&["Label:n4", "VBoxContainer"]));
    let framed = kid(&h.root, 1);
    check_eq!(descs(&framed), strs(&["Label:inner", "VBoxContainer"]));
    h.state.x = 2;
    h.run();
    check_eq!(descs(&h.root)[0], "Label:n6");
    Ok(())
}

struct CondState {
    opt: Option<i32>,
    got: Cell<i32>,
    not_pressed: Cell<u32>,
}
#[tree]
fn cond_view(s: &CondState) {
    Button..{
        if ON(pressed)
            && let Some(v) = s.opt
        {
            s.got.set(v)
        }
        if !ON(pressed) {
            s.not_pressed.set(s.not_pressed.get() + 1)
        }
    };
}
fn on_in_conditions(parent: &Gd<Node>) -> Result<(), String> {
    let state = CondState {
        opt: Some(7),
        got: Cell::new(0),
        not_pressed: Cell::new(0),
    };
    let mut h = Harness::new(parent, state, |b, s| cond_view(b, s, &mut |_, _| {}));
    h.run();
    h.run();
    check_eq!(h.state.got.get(), 0);
    check_eq!(h.state.not_pressed.get(), 2);
    kid(&h.root, 0).emit_signal("pressed", &[]);
    check!(h.pump());
    check_eq!(h.state.got.get(), 7);
    check_eq!(h.state.not_pressed.get(), 2);
    h.state.opt = None;
    kid(&h.root, 0).emit_signal("pressed", &[]);
    check!(h.pump());
    check_eq!(h.state.got.get(), 7);
    Ok(())
}

// a function lending its body a borrow of its own state
#[tree]
fn lends(body: Body<Node, &str>) {
    let text = STATE(String::from("lent"));
    body(&text);
}
#[tree]
fn lends_two(body: Body<VBoxContainer, (&str, &[i32])>) {
    let numbers = vec![1, 2, 3];
    VBoxContainer..{ body("two", &numbers) };
}
#[tree]
fn lend_view() {
    lends()..|t| {
        Label..{
            UPDATE(text = t.to_string());
        };
    };
    lends_two()..|name, numbers| {
        Label..{
            UPDATE(text = format!("{name} {numbers:?}"));
        };
    };
    #[tree]
    let lend = |body: Body<Node, (&str, &[i32])>| {
        let local = format!("from a {}", "closure");
        body(&local, &[])
    };
    lend()..|t, _| {
        Label..{
            UPDATE(text = t.to_string());
        };
    };
}
fn bodies_borrow_locals(parent: &Gd<Node>) -> Result<(), String> {
    let mut h = Harness::new(parent, (), |b, _| lend_view(b, &mut |_, _| {}));
    h.run();
    check_eq!(
        descs(&h.root),
        strs(&["Label:lent", "VBoxContainer", "Label:from a closure"])
    );
    check_eq!(descs(&kid(&h.root, 1)), strs(&["Label:two [1, 2, 3]"]));
    Ok(())
}

#[tree]
fn early(v: bool) {
    Button..{
        INIT(text = "b");
    };
    if v {
        return;
    }
    Label..{
        INIT(text = "skipped");
    };
}
#[tree]
fn early_view(s: &IfState) {
    early(s.show)..{};
    Label..{
        INIT(text = "after");
    };
}
fn early_return_in_call(parent: &Gd<Node>) -> Result<(), String> {
    let mut h = Harness::new(parent, IfState { show: false }, |b, s| {
        early_view(b, s, &mut |_, _| {})
    });
    h.run();
    check_eq!(
        descs(&h.root),
        strs(&["Button:b", "Label:skipped", "Label:after"])
    );
    let after = id(&kid(&h.root, 2));
    // `early` returns before its label: the caller's `after` must stay itself
    h.state.show = true;
    h.run();
    check_eq!(descs(&h.root), strs(&["Button:b", "Label:after"]));
    check_eq!(id(&kid(&h.root, 1)), after);
    h.state.show = false;
    h.run();
    check_eq!(
        descs(&h.root),
        strs(&["Button:b", "Label:skipped", "Label:after"])
    );
    check_eq!(id(&kid(&h.root, 2)), after);
    Ok(())
}

#[tree]
fn wrap(body: Body<VBoxContainer>) {
    VBoxContainer..{
        body();
        Label..{
            INIT(text = "wrap tail");
        };
    };
}
#[tree]
fn wrap_view(s: &IfState) {
    wrap()..{
        Label..{
            INIT(text = "first");
        };
        if s.show {
            return;
        }
        Label..{
            INIT(text = "second");
        };
    };
}
fn early_return_in_body(parent: &Gd<Node>) -> Result<(), String> {
    let mut h = Harness::new(parent, IfState { show: false }, |b, s| {
        wrap_view(b, s, &mut |_, _| {})
    });
    h.run();
    let vbox = kid(&h.root, 0);
    check_eq!(
        descs(&vbox),
        strs(&["Label:first", "Label:second", "Label:wrap tail"])
    );
    let tail = id(&kid(&vbox, 2));
    h.state.show = true;
    h.run();
    check_eq!(descs(&vbox), strs(&["Label:first", "Label:wrap tail"]));
    check_eq!(id(&kid(&vbox, 1)), tail);
    Ok(())
}
