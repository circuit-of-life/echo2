# echo_export

Exports what echo's `#[tree]` macro parses, for every tree function in a project, as JSON: for
analysis and automated porting.

```sh
cargo run -p echo_export -- path/to/project/src --out export
python3 echo_export/summary.py export     # an example consumer: nodes, props and signals
```

It reads the source files (the project doesn't need to build) and parses each `#[tree]`
function's body (free functions and methods, also nested ones) with the macro's own parser,
[`echo_macro/src/tree.rs`](../echo_macro/src/tree.rs), included as a module: what it reports is
what the macro sees. `ON(..)` and `body(..)` are left as written (the macro rewrites them before
parsing), so they show up in the code they're in.

It writes two files:

- **`tree.json`**: the structure: elements, with what the macro knows about them (node types,
  property names, signal names, callees, keys, ..).
- **`code.json`**: every piece of Rust the macro doesn't look into (expressions, statements,
  patterns, types), by key, with its file, line, column and original text.

## Reading it from Rust

[`echo_export::schema`](src/schema.rs) has the format as Rust types, which serde reads both files
into (the exporter writes them from the same types):

```toml
echo_export = { path = "../echo_export", default-features = false }   # only serde, no parser
```

```rust
use echo_export::schema::{Element, Export, walk};

let export = Export::load("export")?;
for function in &export.tree.functions {
    walk(&function.body, &mut |e| {
        if let Element::Init { props, .. } | Element::Update { props, .. } = e {
            for prop in props {
                println!("{} = {}", prop.name, export.code(&prop.value).text);
            }
        }
    });
}
```

`Element` is an enum of the kinds below, `CodeRef` reads either form of a code reference (as a
key and its signals), `walk` visits elements depth first, and `Element::bodies` gives what's
nested in one.

## `tree.json`

```json
{ "version": 1, "functions": [function, ..] }
```

A **function**:

```json
{
  "file": "src/ui/menus.rs", "line": 20,
  "name": "confirm_button",            // methods: "Type::name"
  "in": "Control",                     // #[tree(in Control)], or null
  "params": [code, ..],                // the parameters, without the body parameter
  "body_param": body_param or null,
  "returns": code or null,
  "body": [element, ..]
}
```

A **body_param** (`body: Body<N, A>`): `{ "name": "body", "node": "VBoxContainer" or null, "args": code or null }`.

**code** is a key into `code.json` (`"c42"`), or, when the code has `ON(..)`s in it,
`{ "code": "c42", "signals": ["pressed"] }`.

**Elements**, by `kind` (all have a `line`, except `block`):

| kind | from | fields |
|---|---|---|
| `node` | `Label..{ .. };` | `node` (the type, as written), `body` |
| `let_node` | `let a = Label..{ .. };` | `pat`: code, `node`, `body` |
| `init`, `update` | `INIT(..);`, `UPDATE(..);` | `props`: `[{ "name", "args": [code] (only for `name(args) = v`), "value": code }]` |
| `code` | any other statement or item | `code`: key, `signals` (if it has `ON(..)`s), `body_call: true` and `args: [code]` (if it's `body(args)`) |
| `call`, `let_call` | `f(args)..{ .. };`, `let v = f(..)..{ .. };` | `pat` (let_call), `callee` (as written; `receiver.method` for method calls), `args: [code]`, `body_params`: `null` or `{ "move", "pats": [code] }` (for `..\|a, b\| { .. }`), `body` |
| `tree_closure` | `#[tree] let row = \|..\| { .. };` | `name`: code, `in`, `params`, `body_param`, `body` |
| `if` | `if c { .. } else if .. { .. } else { .. }` | `branches: [{ "cond": code, "body" }]`, `else`: elements or null |
| `match` | `match v { .. }` | `value`: code, `arms: [{ "pat": code, "guard": code or null, "body" }]` |
| `for` | `for x in xs { KEY(k); .. }` | `pat`, `iter`, `key` (code or null), `body` |
| `state` | `let mut x = STATE(init);` | `name`, `mut`, `type` (code or null), `init`: code |
| `memo` | `MEMO(v) >> { .. };` | `value`: code, `body` |
| `block` | `{ .. }` | `body` |
| `error` | an element the macro can't parse | `message` |

What an element applies to follows from where it is: `init`/`update` and `ON(..)` apply to the
innermost `node` around them; in a call's body, to the callee's `body_param.node`; at a
function's top level, to its `in` node (see `summary.py`).

## `code.json`

```json
{ "version": 1, "code": { "c42": { "file": "src/ui/menus.rs", "line": 35, "column": 11,
  "text": "s.count += 1", "signals": ["pressed"] }, .. } }
```

`text` is the original source, formatting and comments included. `line` is 1-based, `column`
1-based in characters. `signals` is only there if the code has `ON(..)`s in it.
