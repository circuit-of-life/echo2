#!/usr/bin/env python3
"""Measures rust-analyzer's latency after an edit inside a big `#[tree]` function.

    python3 echo_tests/ra_edit_latency.py [workspace_root] [edits]

Talks LSP to `rust-analyzer` like an editor would: opens a generated 3300-line file (four ~800
line tree functions), waits until indexing is done, then repeatedly edits a string literal in the
middle of one function and times how long semantic highlighting and inlay hints for the screen
around the edit take to come back (what "intellisense catching up" looks like in the editor).
"""
import json, os, subprocess, sys, threading, time, queue

here = os.path.dirname(os.path.abspath(__file__))
root = os.path.abspath(sys.argv[1]) if len(sys.argv) > 1 else os.path.dirname(here)
edits = int(sys.argv[2]) if len(sys.argv) > 2 else 6
big = os.path.join(root, "echo", "examples", "big.rs")
os.makedirs(os.path.dirname(big), exist_ok=True)
uri = "file://" + big

if os.environ.get("BIG_SRC"):
    src = open(os.environ["BIG_SRC"]).read()
else:
    src = subprocess.run(
        [sys.executable, os.path.join(here, "gen_big.py")], capture_output=True, text=True,
        check=True,
    ).stdout
with open(big, "w") as f:
    f.write(src)
lines = src.split("\n")
# a line in the middle of big_1
start = next(i for i, l in enumerate(lines) if l.startswith("pub fn big_1"))
target = next(i for i in range(start + 400, len(lines)) if '"Title' in lines[i])

proc = subprocess.Popen(
    ["rust-analyzer"], cwd=root, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
    stderr=open(os.environ["RA_STDERR"], "w") if os.environ.get("RA_STDERR") else subprocess.DEVNULL,
)
inbox = queue.Queue()


def reader():
    f = proc.stdout
    while True:
        headers = {}
        while True:
            line = f.readline()
            if not line:
                return
            line = line.decode().strip()
            if not line:
                break
            k, v = line.split(":", 1)
            headers[k.lower()] = v.strip()
        body = f.read(int(headers["content-length"]))
        inbox.put(json.loads(body))


threading.Thread(target=reader, daemon=True).start()
next_id = [0]


def send(msg):
    data = json.dumps(msg).encode()
    proc.stdin.write(f"Content-Length: {len(data)}\r\n\r\n".encode() + data)
    proc.stdin.flush()


def request(method, params):
    next_id[0] += 1
    send({"jsonrpc": "2.0", "id": next_id[0], "method": method, "params": params})
    return next_id[0]


def notify(method, params):
    send({"jsonrpc": "2.0", "method": method, "params": params})


state = {"quiescent": False}


def pump(until_ids=(), timeout=600):
    """Handles server messages until all `until_ids` have responses. Returns them."""
    want = set(until_ids)
    got = {}
    deadline = time.time() + timeout
    while want - got.keys():
        try:
            msg = inbox.get(timeout=max(0.01, deadline - time.time()))
        except queue.Empty:
            raise TimeoutError(f"no response for {want - got.keys()}")
        if "id" in msg and "method" in msg:  # server -> client request
            result = None
            if msg["method"] == "workspace/configuration":
                result = [settings for _ in msg["params"]["items"]]
            send({"jsonrpc": "2.0", "id": msg["id"], "result": result})
        elif "id" in msg:
            if "error" in msg:
                raise RuntimeError(msg["error"])
            got[msg["id"]] = msg.get("result")
        elif msg.get("method") == "experimental/serverStatus":
            state["quiescent"] = msg["params"].get("quiescent", False)
    return got


settings = {
    "checkOnSave": False,
    "check": {"workspace": False},
    "cachePriming": {"enable": False},
    "procMacro": {"enable": True},
    "cargo": {"buildScripts": {"enable": True}},
}
token_types = ["namespace", "type", "class", "enum", "interface", "struct", "typeParameter",
               "parameter", "variable", "property", "enumMember", "event", "function", "method",
               "macro", "keyword", "modifier", "comment", "string", "number", "regexp", "operator"]
rid = request("initialize", {
    "processId": os.getpid(),
    "rootUri": "file://" + root,
    "initializationOptions": settings,
    "capabilities": {
        "textDocument": {
            "semanticTokens": {"requests": {"full": True, "range": True}, "tokenTypes": token_types,
                               "tokenModifiers": [], "formats": ["relative"]},
            "inlayHint": {},
            "publishDiagnostics": {},
            "synchronization": {"didSave": True},
        },
        "window": {"workDoneProgress": True},
        "workspace": {"configuration": True},
        "experimental": {"serverStatusNotification": True},
    },
})
pump([rid])
notify("initialized", {})
notify("textDocument/didOpen", {
    "textDocument": {"uri": uri, "languageId": "rust", "version": 1, "text": src},
})


def drain_until_quiescent(timeout=900):
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            msg = inbox.get(timeout=1)
        except queue.Empty:
            if state["quiescent"]:
                return
            continue
        if "id" in msg and "method" in msg:
            result = [settings for _ in msg["params"]["items"]] if msg["method"] == "workspace/configuration" else None
            send({"jsonrpc": "2.0", "id": msg["id"], "result": result})
        elif msg.get("method") == "experimental/serverStatus":
            state["quiescent"] = msg["params"].get("quiescent", False)
    raise TimeoutError("never became quiescent")


def screen(line):
    return {"start": {"line": max(0, line - 30), "character": 0},
            "end": {"line": line + 30, "character": 0}}


def measure(version):
    t = time.perf_counter()
    a = request("textDocument/semanticTokens/range", {"textDocument": {"uri": uri}, "range": screen(target)})
    b = request("textDocument/inlayHint", {"textDocument": {"uri": uri}, "range": screen(target)})
    pump([a, b])
    return time.perf_counter() - t


try:
    t0 = time.perf_counter()
    drain_until_quiescent()
    t_index = time.perf_counter() - t0
    t_first = measure(1)
    samples = []
    text = src
    for i in range(edits):
        old = lines[target]
        new = old.replace('"Title', f'"Title{i} ', 1)
        lines[target] = new
        notify("textDocument/didChange", {
            "textDocument": {"uri": uri, "version": i + 2},
            "contentChanges": [{
                "range": {"start": {"line": target, "character": 0},
                          "end": {"line": target, "character": len(old)}},
                "text": new,
            }],
        })
        samples.append(measure(i + 2))
    samples.sort()
    print(f"rust-analyzer on {root}")
    print(f"  initial load+index      {t_index:6.2f}s")
    print(f"  first highlight+hints   {t_first:6.2f}s")
    print(f"  after an edit (median)  {samples[len(samples) // 2]:6.2f}s   min {samples[0]:.2f}s  max {samples[-1]:.2f}s")
finally:
    try:
        rid = request("shutdown", None)
        pump([rid], timeout=10)
        notify("exit", None)
    except Exception:
        pass
    proc.kill()
    os.remove(big)
