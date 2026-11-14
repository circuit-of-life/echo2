# echo

Immediate-mode-style UI in Rust, over any retained node tree: write a function that describes
the tree for the current state, run it every frame, and echo creates, updates, reorders and
removes nodes to match, keeping node identity (and `STATE`) for elements that keep running.

```rust
#[tree(in Control)]
fn ui(s: &mut AppState) {
    VBoxContainer..{
        Label..{
            UPDATE(text = format!("{} clicks", s.clicks));
        };
        Button..{
            INIT(text = "click");
            if ON(pressed) {
                s.clicks += 1
            }
        };
        for item in s.items.iter() {
            KEY(item.id);
            Label..{
                UPDATE(text = item.name.clone());
            };
        }
    };
}
```

The DSL is documented on the `#[tree]` attribute (hover it in your editor). Coming from the
old version? See [MIGRATION.md](MIGRATION.md).

## Backends

echo doesn't know what the nodes are. A **backend** does: it implements [`Backend`](echo/src/backend.rs) for the
tree operations, and an `Element` type for each kind of node the tree code names
(`Label..{}`, `#[tree(in Control)]`).

- `Backend` works on opaque `NodeId`s: put a child at an index, ask for a child's index, remove a
  node, connect and disconnect events by name, and ask for a rerun when an event fires.
  echo calls it through `dyn`, so its runtime is compiled once, whatever the backend.
- `Element` says which backend a node type belongs to and what handle `node()` and
  `let a = Label..{}` give. `Create` makes one for `Label..{}`, and `Is<T>` says it can be used
  where a `T` is expected (`impl Is<Control> for Label {}`), for `in T` functions and bodies.
- Properties are setter calls on that handle: `UPDATE(text = x)` is `handle.set_text(x)`, and
  `UPDATE(theme_color("font") = c)` is `handle.set_theme_color("font", c)`. The backend's handle
  types decide which properties exist and what they take, so a misspelled property or a wrong
  type is a compile error at the property's name.

[`echo::memory`](echo/src/memory.rs) is a backend that keeps the tree in memory, with a few
element types (`Label`, `Button`, `LineEdit`, `HSlider`, containers). It's what echo's own tests
run on, and it works for testing your tree code too.

There's no Godot backend on this branch yet; `echo_tests` and `echo_examples` are the Godot
versions of the tests and examples, left out of the workspace until there is one.

## Running it

```rust
let app = App::<Root>::new(backend.clone(), root_id);

// every frame, and again (with `true`) when the backend's `request_rerun` is called
app.run(force_memo, |b| ui(b.upcast(), &mut state, &mut |_, _| {}));
```

A tree function gets the builder first and a body last (here an empty one). The view is passed
to every `run`, so it borrows whatever that run has: several fields, another object's state, or
a context that's only lent to a callback. `App` is a cheap handle (`Clone`).

When an event an `ON(..)` waits for fires, echo calls the backend's `request_rerun`. Run the app
with `force_memo = true` then, and `ON(event)` is `Some(args)` during that run, where it's
checked. Events fired *during* a run (e.g. setting a slider's value makes it fire
`value_changed`) are ignored, and calling `run` while running does nothing, so this can't
re-enter.

## Tests

```sh
cargo test        # echo's behavior on the in-memory backend, and the macro's parsing/errors
```
