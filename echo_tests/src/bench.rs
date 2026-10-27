use std::{cell::Cell, time::Instant};

use echo::tree;
use godot::{
    classes::{Button, Control, HBoxContainer, Label, VBoxContainer},
    prelude::*,
};

use crate::util::*;

struct Row {
    id: u64,
    name: String,
    value: i32,
}
struct Rows {
    rows: Vec<Row>,
    clicks: Cell<u32>,
}
fn rows(n: u64) -> Rows {
    Rows {
        rows: (0..n)
            .map(|id| Row {
                id,
                name: format!("row {id}"),
                value: id as i32 * 3,
            })
            .collect(),
        clicks: Cell::new(0),
    }
}

#[tree]
fn rows_view(s: &Rows) {
    VBoxContainer..{
        for row in s.rows.iter() {
            KEY(row.id);
            HBoxContainer..{
                Label..{
                    UPDATE(text = row.name.clone());
                };
                Label..{
                    INIT(horizontal_alignment = 2);
                    UPDATE(text = row.value.to_string(), visible = row.value >= 0);
                };
                Button..{
                    INIT(text = "x");
                    if ON(pressed) {
                        s.clicks.set(s.clicks.get() + 1)
                    }
                };
            };
        }
    };
}

#[tree]
fn rows_memo_view(s: &Rows) {
    VBoxContainer..{
        for row in s.rows.iter() {
            KEY(row.id);
            MEMO((row.name.clone(), row.value)) >> {
                HBoxContainer..{
                    Label..{
                        UPDATE(text = row.name.clone());
                    };
                    Label..{
                        INIT(horizontal_alignment = 2);
                        UPDATE(text = row.value.to_string(), visible = row.value >= 0);
                    };
                    Button..{
                        INIT(text = "x");
                        if ON(pressed) {
                            s.clicks.set(s.clicks.get() + 1)
                        }
                    };
                };
            };
        }
    };
}

#[tree]
fn deep_view(depth: u32) {
    if depth > 0 {
        Control..{
            Label..{
                UPDATE(text = depth.to_string());
            };
            deep_view(depth - 1)..{};
        };
    }
}
struct Deep {
    copies: u64,
    depth: u32,
}
#[tree]
fn deep_root(s: &Deep) {
    for i in 0..s.copies {
        KEY(i);
        deep_view(s.depth)..{};
    }
}

fn time_us(mut f: impl FnMut()) -> f64 {
    let t = Instant::now();
    f();
    t.elapsed().as_secs_f64() * 1e6
}

fn report(name: &str, mut samples: Vec<f64>) {
    samples.sort_by(f64::total_cmp);
    let median = samples[samples.len() / 2];
    let min = samples[0];
    println!("{name:<34} median {median:>10.1} us   min {min:>10.1} us");
}

fn bench_rows(parent: &Gd<Node>, memo: bool) {
    let n = 1000;
    let tag = if memo { "rows_memo" } else { "rows" };
    let mut h = if memo {
        Harness::new(parent, rows(n), |b, s| rows_memo_view(b, s, &mut |_, _| {}))
    } else {
        Harness::new(parent, rows(n), |b, s| rows_view(b, s, &mut |_, _| {}))
    };
    report(&format!("{tag}[{n}] create"), vec![time_us(|| h.run())]);

    let samples = (0..40).map(|_| time_us(|| h.run())).collect();
    report(&format!("{tag}[{n}] rerun, nothing changed"), samples);

    let samples = (0..40)
        .map(|i| {
            h.state.rows[(i * 37) % n as usize].value += 1;
            time_us(|| h.run())
        })
        .collect();
    report(&format!("{tag}[{n}] rerun, one row changed"), samples);

    let samples = (0..40)
        .map(|i| {
            let vbox = kid(&h.root, 0);
            let mut button = kid(&kid(&vbox, (i * 13) % n as usize), 2);
            button.emit_signal("pressed", &[]);
            time_us(|| assert!(h.pump()))
        })
        .collect();
    report(&format!("{tag}[{n}] signal-triggered rerun"), samples);
    assert_eq!(h.state.clicks.get(), 40);

    let samples = (0..10)
        .map(|i| {
            h.state.rows.rotate_left(1 + i);
            time_us(|| h.run())
        })
        .collect();
    report(&format!("{tag}[{n}] rerun, rows rotated"), samples);
}

fn bench_deep(parent: &Gd<Node>) {
    let mut h = Harness::new(
        parent,
        Deep {
            copies: 20,
            depth: 50,
        },
        |b, s| deep_root(b, s, &mut |_, _| {}),
    );
    report("deep[20x50] create", vec![time_us(|| h.run())]);
    let samples = (0..40).map(|_| time_us(|| h.run())).collect();
    report("deep[20x50] rerun, nothing changed", samples);
}

pub fn run_all(parent: &Gd<Node>, filter: Option<&str>) {
    println!(
        "profile: {}",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );
    let want = |name: &str| filter.is_none_or(|f| name.contains(f));
    if want("rows") {
        bench_rows(parent, false);
    }
    if want("memo") {
        bench_rows(parent, true);
    }
    if want("deep") {
        bench_deep(parent);
    }
    if want("micro") {
        micro(parent);
    }
}

/// Costs of the individual engine operations echo performs per node, per run.
pub fn micro(parent: &Gd<Node>) {
    use godot::classes::Object;
    let n = 20_000;
    let mut host = Node::new_alloc();
    parent.clone().add_child(&host);
    let mut label = Label::new_alloc();
    host.add_child(&label);
    let as_node: Gd<Node> = label.clone().upcast();
    let per = |name: &str, us: f64| println!("{name:<44} {:>8.1} ns/op", us * 1000.0 / n as f64);

    per(
        "Gd<Node>::cast::<Label>()",
        time_us(|| {
            for _ in 0..n {
                std::hint::black_box(as_node.clone().cast::<Label>());
            }
        }),
    );
    per(
        "Gd::clone()",
        time_us(|| {
            for _ in 0..n {
                std::hint::black_box(as_node.clone());
            }
        }),
    );
    per(
        "is_instance_valid()",
        time_us(|| {
            for _ in 0..n {
                std::hint::black_box(as_node.is_instance_valid());
            }
        }),
    );
    per(
        "get_index()",
        time_us(|| {
            for _ in 0..n {
                std::hint::black_box(as_node.get_index());
            }
        }),
    );
    per(
        "move_child() to same index",
        time_us(|| {
            for _ in 0..n {
                host.move_child(&as_node, 0);
            }
        }),
    );
    per(
        "set(\"text\", &str.to_variant())",
        time_us(|| {
            for _ in 0..n {
                label
                    .upcast_mut::<Object>()
                    .set("text", &"hello".to_variant());
            }
        }),
    );
    let sname = StringName::from("text");
    per(
        "set(cached StringName, &str.to_variant())",
        time_us(|| {
            for _ in 0..n {
                label
                    .upcast_mut::<Object>()
                    .set(&sname, &"hello".to_variant());
            }
        }),
    );
    let v = "hello".to_variant();
    per(
        "set(cached StringName, cached Variant)",
        time_us(|| {
            for _ in 0..n {
                label.upcast_mut::<Object>().set(&sname, &v);
            }
        }),
    );
    per(
        "set_text(\"hello\") (typed setter)",
        time_us(|| {
            for _ in 0..n {
                label.set_text("hello");
            }
        }),
    );
    per(
        "get(cached StringName) (for compare)",
        time_us(|| {
            for _ in 0..n {
                std::hint::black_box(label.upcast_ref::<Object>().get(&sname));
            }
        }),
    );
    per(
        "StringName::from(\"text\")",
        time_us(|| {
            for _ in 0..n {
                std::hint::black_box(StringName::from("text"));
            }
        }),
    );
    host.queue_free();
}
