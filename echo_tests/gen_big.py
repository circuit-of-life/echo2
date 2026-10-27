#!/usr/bin/env python3
"""Generates a large, realistic `#[tree]` file for measuring compile times.

    python3 echo_tests/gen_big.py > echo/examples/big.rs
    touch echo/examples/big.rs && time cargo build --example big
"""
import sys

FNS = int(sys.argv[1]) if len(sys.argv) > 1 else 4
SECTIONS = int(sys.argv[2]) if len(sys.argv) > 2 else 12

out = []
w = out.append
w("""#![allow(unused)]
use std::cell::Cell;

use echo::{Body, tree};
use godot::{
    classes::{Button, Control, HBoxContainer, Label, PanelContainer, VBoxContainer},
    prelude::*,
};

macro_rules! __echo_deny {
    ($($t:tt)*) => {};
}
pub(crate) use __echo_deny;

pub struct State {
    pub flag: bool,
    pub n: u32,
    pub mode: u8,
    pub count: Cell<i32>,
    pub items: Vec<(u64, String)>,
}

#[tree]
fn card(title: &str, body: Body<VBoxContainer, i32>) {
    PanelContainer..{
        VBoxContainer..{
            Label..{
                INIT(text = title.to_string());
            };
            body(1)
        };
    };
}
""")


def section(ind, k):
    p = " " * ind
    return f"""{p}Label..{{
{p}    INIT(text = "Title {k}", theme(color, font_color) = Color::RED);
{p}    UPDATE(visible = s.flag);
{p}}};
{p}HBoxContainer..{{
{p}    INIT(theme(constant, separation) = 4);
{p}    Button..{{
{p}        INIT(text = "inc {k}");
{p}        if ON(pressed) {{
{p}            s.count.set(s.count.get() + {k})
{p}        }}
{p}    }};
{p}    Button..{{
{p}        INIT(text = "dec {k}");
{p}        UPDATE(disabled = s.count.get() <= 0);
{p}        if ON(pressed) {{
{p}            s.count.set(s.count.get() - 1)
{p}        }}
{p}    }};
{p}}};
{p}if s.flag {{
{p}    PanelContainer..{{
{p}        Label..{{
{p}            UPDATE(text = format!("{k}: {{}}", s.count.get()));
{p}        }};
{p}    }};
{p}}} else if s.n > {k} {{
{p}    Label..{{
{p}        INIT(text = "big");
{p}    }};
{p}}} else {{
{p}    Control..{{}};
{p}}}
{p}for (id, name) in s.items.iter() {{
{p}    KEY(*id);
{p}    HBoxContainer..{{
{p}        Label..{{
{p}            UPDATE(text = name.clone());
{p}        }};
{p}        Button..{{
{p}            INIT(text = "x");
{p}            if ON(pressed) {{
{p}                s.count.set(*id as i32)
{p}            }}
{p}        }};
{p}    }};
{p}}}
{p}MEMO(s.n) >> {{
{p}    VBoxContainer..{{
{p}        let clicks = STATE(0i32);
{p}        Label..{{
{p}            UPDATE(text = format!("memo {k} {{}} {{}}", s.n, *clicks));
{p}        }};
{p}    }};
{p}}};
{p}match s.mode {{
{p}    0 => {{
{p}        Label..{{
{p}            INIT(text = "mode zero");
{p}        }};
{p}    }}
{p}    _ => {{
{p}        Button..{{
{p}            INIT(text = "other");
{p}        }};
{p}    }}
{p}}}
{p}card("card {k}")..|x| {{
{p}    Label..{{
{p}        UPDATE(text = format!("{{x}} {{}}", s.n));
{p}    }};
{p}}};
"""


for f in range(FNS):
    w(f"\n#[tree]\npub fn big_{f}(s: &State) {{\n")
    # nest sections a few containers deep, like a real screen layout
    depth = 0
    for k in range(SECTIONS):
        if k % 3 == 0 and depth < 4:
            w("    " * (depth + 1) + "VBoxContainer..{\n")
            depth += 1
        w(section(4 * (depth + 1), k))
    while depth > 0:
        w("    " * depth + "};\n")
        depth -= 1
    w("}\n")

calls = "".join(f"            big_{f}(b.clone(), s, &mut |_, _| {{}});\n" for f in range(FNS))
w(f"""
fn main() {{
    // reachable from main, otherwise rustc never codegens the tree functions
    if std::hint::black_box(false) {{
        let mut app = echo::App::new(Node::new_alloc(), |b, s: &mut State| {{
{calls}        }});
        let mut state = State {{ flag: false, n: 0, mode: 0, count: Cell::new(0), items: vec![] }};
        app.run(&mut state, false);
    }}
}}
""")
sys.stdout.write("".join(out))
