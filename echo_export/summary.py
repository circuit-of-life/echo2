#!/usr/bin/env python3
"""An example consumer of echo_export's output: the node types used, the INIT/UPDATE properties
set on each, and the signals ON(..) listens to on each.

Unlike a text search, it knows what a call's body is placed in: the callee's `Body<N>` (if the
callee is one of the exported tree functions).

    python3 echo_export/summary.py <export dir>
"""

import json
import sys
from collections import Counter, defaultdict

out = sys.argv[1] if len(sys.argv) > 1 else "echo_export"
tree = json.load(open(f"{out}/tree.json"))
functions = {f["name"].split("::")[-1]: f for f in tree["functions"]}
nodes = defaultdict(lambda: {"count": 0, "props": defaultdict(set), "signals": Counter()})


def short(path):
    return path.split("::")[-1].strip()


def signals(ref):
    return ref.get("signals", []) if isinstance(ref, dict) else []


def body_node(call):
    callee = functions.get(short(call["callee"]))
    if callee is None:
        return f"(body of {call['callee']}, not a tree function found)"
    param = callee["body_param"]
    if param is None or param["node"] is None:
        return f"(body of {call['callee']}: any node)"
    return short(param["node"])


def walk(elems, current):
    """`current`: the node type the elements are in (what ON/INIT/UPDATE apply to)."""
    for e in elems:
        kind = e["kind"]
        if kind in ("node", "let_node"):
            node = short(e["node"])
            nodes[node]["count"] += 1
            walk(e["body"], node)
        elif kind in ("init", "update"):
            for prop in e["props"]:
                nodes[current]["props"][prop["name"]].add(kind.upper())
                for s in signals(prop["value"]):
                    nodes[current]["signals"][s] += 1
        elif kind == "code":
            for s in e.get("signals", []):
                nodes[current]["signals"][s] += 1
        elif kind in ("call", "let_call"):
            walk(e["body"], body_node(e))
        elif kind == "if":
            for branch in e["branches"]:
                for s in signals(branch["cond"]):
                    nodes[current]["signals"][s] += 1
                walk(branch["body"], current)
            walk(e["else"] or [], current)
        elif kind == "match":
            for arm in e["arms"]:
                walk(arm["body"], current)
        elif kind in ("for", "memo", "block"):
            walk(e["body"], current)
        elif kind == "tree_closure":
            walk(e["body"], short(e["in"]) if e["in"] else "(the node a tree closure is used in)")


for f in tree["functions"]:
    walk(f["body"], short(f["in"]) if f["in"] else "(the node a function is used in)")

for name in sorted(nodes, key=lambda n: (n.startswith("("), n)):
    n = nodes[name]
    print(name + (f"  ({n['count']}x)" if n["count"] else ""))
    if n["props"]:
        props = ", ".join(f"{p} [{'/'.join(sorted(k))}]" for p, k in sorted(n["props"].items()))
        print("  props:   " + props)
    if n["signals"]:
        print("  signals: " + ", ".join(f"{s} ({c}x)" for s, c in sorted(n["signals"].items())))
