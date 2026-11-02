# Migrating to the reworked echo

This covers everything that changed between the old crate (`main`, `6b9996a`) and the rework
(`echo-rework`): what you write differently, and what behaves differently even if you don't.

Most of the DSL is the same. The changes are concentrated in four places: how tree functions take
a body and pass values around, signal handlers (`ON`), state (`STATE`), and `{ }` blocks. Every
removed form is a compile error that says what replaced it, so the compiler can lead the way.

## At a glance

| Old | New |
|---|---|
| `#[tree(Node())]` | `#[tree]` |
| `#[tree(VBoxContainer(i32) in PanelContainer)]` | `#[tree(in PanelContainer)]` + a `body: Body<VBoxContainer, i32>` parameter |
| `BODY(42);` | `body(42);` |
| `f(x)..{ let (a, b) = ARGS; .. };` | `f(x)..\|a, b\| { .. };` |
| tree functions return a `Builder` | they return their own value: `let v = f(x)..{};` |
| `ON(pressed = \|_\| ..);` | `if ON(pressed) { .. }` |
| `ON(value_changed = \|args\| ..);` | `if let Some(args) = ON(value_changed) { .. }` |
| `STATE(x = \|\| init);` (`Rc<RefCell<T>>`) | `let mut x = STATE(init);` |
| `STATE(*x = \|\| init);` (`Rc<Cell<T>>`) | `let mut x = STATE(init);` |
| `x.get()`, `x.set(v)`, `x.borrow()`, `x.borrow_mut()` | `*x`, `*x = v`, `x`, `x` (it derefs) |
| `{ let n = ..; }` leaks `n` to later elements | `{ }` is a normal block scope; write `let n = ..;` directly |
| `for` always needs `KEY(..)` | only loops that create nodes / state / memos / calls need it |
| `ui(b.upcast(), s, &mut \|b, ()\| b).cast()` | `ui(b.upcast(), s, &mut \|_, _\| {})` |
| `app.run(..)` needs `&mut app` | `app.run(..)` takes `&self`; `App` is `Clone` |

---

## Setting up an `App`

The setup is the same three parts as before: create an `App` in `ready`, run it every frame, and
run it again (with the given `force_memo`) when the root emits `__echo_rerun`. Three things changed.

**The view closure doesn't return a builder any more.** Tree functions return their own value
(usually `()`), and an empty body written by hand is `&mut |_, _| {}`:

```rust
// before
self.app = Some(App::new(root.clone(), |b, s: &mut Foo| {
    ui(b.upcast(), &mut s.app_state, &mut |b, ()| b).cast()
}));

// after
self.app = Some(App::new(root.clone(), |b, s: &mut Foo| {
    ui(b.upcast(), &mut s.app_state, &mut |_, _| {})
}));
```

`App::new` accepts a closure returning anything and ignores the value.

**`run` takes `&self`, and `App` is a cheap `Clone` handle.** There's no need to take the app out
of the state to run it any more:

```rust
// before
let mut app = self.app.take().unwrap();
app.run(self, false);
self.app = Some(app);

// after (the old form still compiles, with an "unused mut" warning)
let app = self.app.clone().unwrap();
app.run(self, false);

// or, with the app next to its state rather than inside it
self.app.as_ref().unwrap().run(&mut self.state, false);
```

**State can be split (new).** If the state lives in several places, make it a tuple and pass
the parts separately: `App<Control, (Game, Settings), Split>`, made with `App::new_split`, runs
with `app.run((&mut game, &mut settings), ..)`. `App<Control, Foo>` works as before.

**Re-entrancy is handled.** Calling `run` while the app is already running logs a warning and
does nothing. Signals emitted *during* a run are ignored, because they come from echo's own
changes: for example, `UPDATE(value = ..)` makes a slider emit `value_changed`. Previously such a
signal re-entered your `__echo_rerun` handler in the middle of a run, which with the `bind_mut()` +
`take().unwrap()` setup means an "already bound" panic, or at best a needless rerun.

Connecting `__echo_rerun` deferred now also works: a signal's event waits until a run reaches its
`ON`, rather than being dropped at the end of the next run.

---

## Tree functions

### The attribute

The attribute now only says where a function may be used. The body's node type and arguments
moved into the signature.

```rust
// before
#[tree(Node())]
fn ui(s: &mut State) { .. }

#[tree(VBoxContainer(i32) in PanelContainer)]
fn card(title: &str) { .. }

// after
#[tree]
fn ui(s: &mut State) { .. }

#[tree(in PanelContainer)]
fn card(title: &str, body: Body<VBoxContainer, i32>) { .. }
```

`#[tree]` means "usable in any node", like `#[tree(Node())]` did. `#[tree(in X)]` is still checked
at compile time.

### The body: `Body` and `body(..)`

The body (what a caller writes after the call) is now an ordinary last parameter, typed
`Body<N, A>`, and you place it by calling it. `N` is the node it must be placed inside (default
`Node`). `A` is what it's given: one type, a tuple for several, or left out for none.

```rust
// before
#[tree(VBoxContainer(i32, String))]
fn pair() {
    VBoxContainer..{
        BODY(1, "one".to_string());
    };
}

// after
use echo::Body;

#[tree]
fn pair(body: Body<VBoxContainer, (i32, String)>) {
    VBoxContainer..{
        body(1, "one".to_string());
    };
}
```

| Body arguments | Old attribute | New parameter | Placed with |
|---|---|---|---|
| none | `X()` | `body: Body<X>` | `body()` |
| one | `X(i32)` | `body: Body<X, i32>` | `body(42)` |
| several | `X(i32, String)` | `body: Body<X, (i32, String)>` | `body(1, s)` |

A function that never places a body doesn't need the parameter; the macro adds an unused one. One
argument is now passed as-is: before, it was a 1-tuple `(a,)`.

The body can be given borrows of the function's own locals and state:

```rust
#[tree]
fn lends(body: Body<Node, &str>) {
    let text = STATE(String::from("hi"));
    body(&text); // any `&str` works, however short-lived
}
```

`body(..)` is also its own scope (see [Identity](#identity-and-early-returns)), and you can place
the body more than once.

### Calling: arguments with `..|a, b|`

```rust
// before
card("Title")..{
    let (n,) = ARGS;
    Label..{ UPDATE(text = n.to_string()); };
};

// after
card("Title")..|n| {
    Label..{ UPDATE(text = n.to_string()); };
};

pair()..|n, text: String| { /* .. */ };   // several, types optional
card("Title")..{ /* .. */ };              // ignoring the arguments, as before
```

`move` works (`..move |a| { .. }`). The braces may go when the body is a single element:
`card("x")..|n| Label..{}`.

### Return values

Tree functions used to return the `Builder`, which callers then threw away. Now they return what
they declare, and the last expression (without `;`) is the value. This replaces passing values
back through `BODY(..)` + `ARGS`:

```rust
// before: the slider tells its body whether it changed
#[tree(HBoxContainer(bool))]
fn slider(value: &mut f64) {
    let mut changed = false;
    HBoxContainer..{ /* .. sets `changed` .. */ BODY(changed); };
}
slider(&mut r)..{
    let (changed,) = ARGS;
    if changed { /* .. */ }
};

// after
#[tree]
fn slider(value: &mut f64) -> bool {
    let mut changed = false;
    HBoxContainer..{ /* .. sets `changed` .. */ };
    changed
}
let changed = slider(&mut r)..{};
if changed { /* .. */ }
```

### Calling a tree function from plain Rust

The generated signature is `fn f(builder, ..your parameters.., body) -> YourReturnType`, where
`body` is your `Body` parameter or the hidden one. From plain Rust (usually just the `App`
closure), pass the builder first and a body last:

```rust
ui(b, &mut state, &mut |_, _| {});
```

### Tree closures (new)

`#[tree]` (or `#[tree(in X)]`) on a `let` makes a closure that works like a tree function and can
use the variables around it. Call it the same way:

```rust
#[tree]
fn view(s: &mut State) {
    #[tree]
    let mut channel = |name: &'static str, value: &mut f64| {
        let changed = slider(value)..{};
        if changed {
            s.last_changed = Some(name)
        }
    };
    channel("Red", &mut s.r)..{};
    channel("Green", &mut s.g)..{};
}
```

They can take a `Body` parameter and declare a return type, like functions.

### Nested items (new)

Tree functions can contain items like any Rust function: `fn`, `struct`, `enum`, `impl`, `use`,
`const`, `static`, `type`, `mod`, `trait`, `macro_rules!`, and nested `#[tree] fn`s.

---

## Signals: `ON`

`ON(signal)` is now an expression: the signal's arguments if it fired during this run
(`Option<..>`), checked like any other condition. Handlers are plain code, not closures.

```rust
// before
Button..{
    ON(pressed = |_| s.count += 1);
};
HSlider..{
    ON(value_changed = |args| s.volume = args[0].to());
};

// after
Button..{
    if ON(pressed) {
        s.count += 1
    }
};
HSlider..{
    if let Some(args) = ON(value_changed) {
        s.volume = args[0].to()
    }
};
```

What's different:

- **`ON(x)` is a `bool` wherever Rust expects one:** as an `if`/`while` condition or match guard,
  or as an operand of `&&`, `||` or `!`. So `if ON(pressed) && let Some(v) = s.selected { .. }`
  works. Elsewhere it's the `Option`: `if let Some(args) = ON(x)`, `let fired = ON(x).is_some();`,
  `f(ON(x))`.
- **`args` is a `SmallVec<[Variant; 4]>`, not a `&[Variant]`.** It derefs to a slice, so
  `args[0]`, `args.len()` and `args.iter()` work. Where a `&[Variant]` is needed, pass `&args`.
- **`return` in a handler returns from the whole tree function**, not just the handler. In a
  function returning `()` this compiles, and silently skips the rest of the view for that run. To
  leave just the handler, use a labeled block:

  ```rust
  if ON(pressed) {
      'h: {
          if !valid { break 'h; }
          s.apply();
      }
  }
  ```

- **`break`, `continue` and `?` act on the surrounding code**, like any `if`. Handlers can also
  move captured values freely.
- **A signal stays connected while its `ON` keeps being checked.** With short-circuiting
  (`ON(a) || ON(b)`), `b` isn't checked on a run where `a` fired. To check both reliably, check them
  into variables first.

A tip that applies to old code too: within a node, check its events *before* the `UPDATE`s that
show what they change. The run a signal causes goes top to bottom, so otherwise it first sets the
node back to the old value. For a text field that also moves the caret.

```rust
LineEdit..{
    if let Some(args) = ON(text_changed) {
        s.input = args[0].to()
    }
    if __builder.node().get_text() != s.input.as_str() {
        UPDATE(text = s.input.clone());
    }
};
```

---

## State: `STATE`

State is declared with `let`, and the variable works like a `&mut` to the value: no `Rc`, `Cell`
or `RefCell`.

```rust
// before
STATE(*count = || 0);              // Rc<Cell<i32>>
STATE(items = || Vec::new());      // Rc<RefCell<Vec<_>>>
count.set(count.get() + 1);
items.borrow_mut().push(x);

// after
let mut count = STATE(0);          // StateMut<i32>
let mut items: Vec<String> = STATE(Vec::new());
*count += 1;
items.push(x);
```

| Old use | New use |
|---|---|
| `x.get()` | `*x` |
| `x.set(v)` | `*x = v` |
| `x.borrow().len()`, `x.borrow_mut().push(..)` | `x.len()`, `x.push(..)` |
| `*x.borrow_mut() = v` | `*x = v` |
| `&*x.borrow()`, passing `x.borrow_mut()` | `&*x`, `&mut *x` (or `&x` / `&mut x`, through deref coercion) |
| `format!("{}", x.get())` | `format!("{x}")` (Display and Debug forward to the value) |

Details:

- **The initializer is the value itself, evaluated once** when the state is created: `STATE(0)`,
  not `STATE(|| 0)`. A type annotation names the value's type: `let x: Vec<u8> = STATE(..)`.
- **The name must be plain:** `let name = STATE(..)` or `let mut name: T = STATE(..)`, not a
  pattern.
- **Watch for `.clone()`.** It used to clone the `Rc` (sharing the state), and now clones the
  value, with no compile error. For state that really has to be shared with something outside the
  tree (a timer callback, ..), put the `Rc` in yourself:
  `let shared = STATE(Rc::new(RefCell::new(0)));` then `shared.clone()` is the `Rc`.
- **A `StateMut` belongs to its run.** Keeping one past the run (in a struct, say) makes the next
  run's `STATE` panic with "still in use". The old `Rc` could be kept and used later; for that, put
  an `Rc` in the state yourself, as above.
- **State still lives as long as its element keeps running**, and comes back fresh after.

---

## `{ }` blocks, `let` and statements

Old `{ }` blocks were inlined into the surrounding code, so `let`s inside them were visible to the
elements after them. Now `{ }` is a normal Rust block scope, and its contents are elements (nodes,
`ON`, `STATE`, ..).

```rust
// before: `text` declared in a block, used after it
{
    let text = format!("n = {}", s.n);
}
Label..{ UPDATE(text = text.clone()); };

// after: declare it directly
let text = format!("n = {}", s.n);
Label..{ UPDATE(text = text.clone()); };
```

If you missed one, rustc points at it: "cannot find value `text` in this scope … the binding
`text` is available in a different scope".

Also new:

- **Plain Rust statements and `let`s are elements**, no braces needed: `s.count += 1;`,
  `let x = ..;`.
- **The last element of a block doesn't need its `;`**, which makes `if ON(p) { s.n += 1 }` work.
- **`while`, `loop`, `unsafe { }` and labeled blocks work as statements.** Their contents are plain
  Rust; only `if`, `match`, `for` and `{ }` contain elements.

---

## Nodes

Unchanged, plus:

- **`let a = Label..{ .. };` binds the node** (`Gd<Label>`) for the code after it. For example, to
  give the input focus back after clicking "Add":
  ```rust
  let mut input = LineEdit..{ /* .. */ };
  Button..{
      if ON(pressed) {
          s.add();
          input.grab_focus()
      }
  };
  ```
- **Paths work as node types:** `godot::classes::Label..{}`.
- **`INIT`/`UPDATE` take raw identifiers:** `INIT(r#type = ..)` sets `type` (it used to set
  `r#type`).
- **`STATE(..)`, `MEMO(..)`, `KEY(..)` and `theme(..)` accept a trailing comma**, as rustfmt adds
  when splitting lines. `INIT`/`UPDATE` always did.

## Loops, branches and memos

- **`for` loops that create nothing with an identity (no nodes, `STATE`, `MEMO` or calls) need no
  `KEY`.** They're plain Rust loops. A loop that does create something and has no `KEY` is an
  error; for identity by index, write `for (i, x) in xs.iter().enumerate() { KEY(i); .. }`.
- **`KEY` now actually decides identity.** See [Behaviour changes](#behaviour-changes).
- **Duplicate keys in one loop** are told apart by order, with a one-time warning.
- **`match` arms can have guards:** `n if n < 0 => { .. }`.
- **`if`/`match`/`MEMO` syntax is unchanged.**

---

## `__builder` and `Builder`

Code that uses `__builder` directly:

- **`__builder.node()`, `.init()`, `.path()` and `.forcing_memos()` work as before.** `node()`
  returns a `Gd<T>` for the node being built.
- **`Builder::cast()` and `upcast()` still exist**, and are now free. `cast()` is checked lazily,
  when `node()` is called.
- **`Builder` is `Clone`.**
- **The `__`-prefixed internals changed.** They take `&self` and return nothing, so patterns like
  `__builder = __builder.__signal(..)` no longer compile. Use the DSL forms instead.
- **The public API is `App`, `Builder`, `Body`, `StateMut`, `PathElem` and `tree`.** The old
  `__Probe` (the "this state is `Copy`, use `Cell`" hint) is gone, along with the forms it was for.
- **`crate::__echo_deny!(Type)` is still required** and still called for every node.

---

## Identity and early returns

Elements are still identified by their position in the code, and by branch or key. Two things
make that sturdier:

- **Every tree function call, and every `body(..)` placement, is its own scope.** A call always
  takes exactly one position in its caller, however many it uses inside. An early `return` (or
  `?`) in a function or a body can no longer shift the identity of what comes after it. Before,
  the caller's next element could take over a skipped element's node and state.
- **`if`/`match` branches with nothing that has an identity compile to a plain `if`/`match`.**
  Branches that do still get their own scope, as before. This only makes things faster.

---

## Behaviour changes

These need no code changes, but your UI behaves differently. They were all bugs in the old
version.

| Situation | Before | Now |
|---|---|---|
| Keyed `for`: an item is removed, inserted or reordered | the rows after it were recreated (nodes, focus and `STATE` lost) | each item keeps its nodes, wherever it moves |
| Element after a `for` whose list changed length | recreated | kept |
| Something before a skipped `MEMO` appears or disappears | children came out in the wrong order | correct order |
| `ON` inside an `if` or `MEMO` | panicked: "no entry found for key" | works |
| An `ON` skipped for a run, then checked again | its signal never fired again | reconnected |
| `UPDATE` makes a node emit a signal (e.g. slider `value`) | re-entered the app mid-run (a `bind_mut` panic with the usual setup) or a needless rerun | ignored: echo's own change, not input |
| A node echo made is freed from outside | next run panicked | recreated |
| `__echo_rerun` connected deferred, `ON` inside a skipped `MEMO` | event lost | event delivered on the next run that reaches it |
| Early `return` in a tree function or body | could hand later elements the wrong nodes | contained |

## Performance

Measured with the crate's benchmarks (`echo_tests`), before and after the rework:

| | Before | After |
|---|---|---|
| Rerun, 1000 rows, nothing changed (release) | 2.7 ms | 0.77 ms |
| Same, each row in a `MEMO` | 0.20 ms | 0.10 ms |
| 1000 rows reordered | 182 ms | 1.5 ms |
| Deep tree, 1000 nested calls | 2.1 ms | 0.40 ms |
| Release build of a 3300-line tree file | 43 s | 4 s |
| rust-analyzer, after an edit in an 800-line tree function | 1.8 s | 0.4 s |

---

## Errors from removed forms

Old forms fail to compile with a message saying what replaced them:

| Old form | Error says |
|---|---|
| `#[tree(VBoxContainer(i32) in P)]` | expected `#[tree]` or `#[tree(in ParentNode)]` … take the body as the last parameter, `body: Body<BodyNode, Args>` |
| `BODY(..)` | `BODY(..)` is gone: take a `body: Body<..>` parameter (the last one) and call it, `body(..)` |
| `let (..) = ARGS;` | `let (..) = ARGS;` is gone: take the arguments as the body's parameters, `func(..)..\|a, b\| { .. }` |
| `ON(signal = handler);` | `ON(signal = handler)` is gone: write the handler as an `if` … |
| `STATE(x = \|\| init);` | state is declared with `let`: `let mut name = STATE(initial value);` |
| `x.get()` / `x.borrow()` on a new state | no method named `get` / `borrow` found for `StateMut<..>` |
