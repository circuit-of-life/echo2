#!/usr/bin/env python3
"""Measures how long rust-analyzer takes to analyze a large generated `#[tree]` file.

    python3 echo_tests/ra_time.py [workspace_root] [runs]

This is a proxy for the latency after an edit inside a big tree function: rust-analyzer then
re-expands the attribute macro on the whole function, and re-lowers and re-infers its body.
`analysis-stats --only big_` does exactly those steps for the generated functions; a run on a
tiny function is subtracted to remove the fixed cost of loading the workspace.
"""
import os, re, subprocess, sys, time

here = os.path.dirname(os.path.abspath(__file__))
root = os.path.abspath(sys.argv[1]) if len(sys.argv) > 1 else os.path.dirname(here)
runs = int(sys.argv[2]) if len(sys.argv) > 2 else 3
big = os.path.join(root, "echo", "examples", "big.rs")
os.makedirs(os.path.dirname(big), exist_ok=True)

with open(big, "w") as f:
    subprocess.run([sys.executable, os.path.join(here, "gen_big.py")], stdout=f, check=True)


def stats(only):
    t = time.perf_counter()
    r = subprocess.run(
        ["rust-analyzer", "analysis-stats", root, "--only", only,
         "--skip-mir-stats", "--skip-data-layout", "--skip-const-eval"],
        capture_output=True, text=True,
    )
    wall = time.perf_counter() - t
    out = r.stdout + r.stderr
    if r.returncode != 0:
        print(out[-3000:])
        sys.exit(1)
    phases = {}
    for name in ("Body lowering", "Inference"):
        m = re.search(rf"{name}:\s*([0-9.]+)(ms|s)", out)
        if m:
            v = float(m.group(1))
            phases[name] = v / 1000 if m.group(2) == "ms" else v
    m = re.search(r"Expressions:\s*(\d+)", out) or re.search(r"exprs: (\d+)", out)
    return wall, phases, out


try:
    # one warm-up so build scripts / proc macros are compiled and cached
    _, _, out = stats("big_0")
    if os.environ.get("RA_DEBUG"):
        print(out)
    best = {}
    for _ in range(runs):
        for only in ("big_", "main"):
            wall, phases, _ = stats(only)
            key = only
            prev = best.get(key)
            if prev is None or wall < prev[0]:
                best[key] = (wall, phases)
    big_wall, big_ph = best["big_"]
    small_wall, small_ph = best["main"]
    print(f"rust-analyzer on {root}")
    for name in ("Body lowering", "Inference"):
        print(f"  {name:<14} big fns {big_ph.get(name, float('nan')):6.2f}s   (tiny fn {small_ph.get(name, float('nan')):5.2f}s)")
    print(f"  wall           big fns {big_wall:6.2f}s   (tiny fn {small_wall:5.2f}s)  -> {big_wall - small_wall:5.2f}s for the 4 big fns")
finally:
    os.remove(big)
