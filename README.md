# echo

Immediate-mode-style UI for Godot in Rust: write a function that describes the node tree for the
current state, run it every frame, and echo creates, updates, reorders and frees nodes to match,
keeping node identity (and `STATE`) for elements that keep running.

```rust
#[tree]
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

Coming from the old version? See [MIGRATION.md](MIGRATION.md).

The DSL is documented on the `#[tree]` attribute (hover it in your editor), and
[`echo_examples`](echo_examples/src) has runnable examples, from the basics to `MEMO`:

```sh
echo_examples/run.sh            # opens a window with all of them
```

## Running it

```rust
// in ready()
self.app = Some(App::new(root.clone(), |b, s: &mut Foo| {
    // a tree function gets the builder first and a body last (here an empty one)
    ui(b.upcast(), &mut s.app_state, &mut |_, _| {})
}));
root.connect("__echo_rerun", &Callable::from_fn("rerun", move |args| {
    let this = &mut *node.bind_mut();
    let app = this.app.clone().unwrap();
    app.run(this, args[0].to::<bool>());
}));

// at the end of process()
let app = self.app.clone().unwrap();
app.run(self, false);
```

`App` is a cheap handle (`Clone`), so it doesn't need to be taken out of the state to run it.
When a signal checked with `ON(..)` fires, the root emits `__echo_rerun(true)`; run the app
with that flag and `ON(signal)` is `Some(args)` during that run, where it's checked. Signals
emitted *during* a run (e.g. setting `value` makes a slider emit `value_changed`) are ignored,
and calling `run` while running does nothing, so this can't re-enter. Connecting
`__echo_rerun` deferred is fine too: an event waits until a run reaches its `ON`.

## Tests and measurements

Everything runs against a real headless Godot (`$GODOT4_BIN`, or `./godot` in this folder):

```sh
echo_tests/run.sh                     # integration tests (echo_tests/src/tests.rs)
echo_examples/run.sh --smoke          # click through every example headlessly
echo_tests/run.sh --release --bench   # runtime benchmarks (echo_tests/src/bench.rs)
cargo test -p echo_macro              # macro parsing/error tests
python3 echo_tests/compile_time.py    # rustc time for a 3300-line generated tree file
python3 echo_tests/ra_time.py         # rust-analyzer time for the same file
```
