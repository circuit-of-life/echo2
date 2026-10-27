#!/usr/bin/env python3
"""Measures how long rustc takes on a large generated `#[tree]` file.

    python3 echo_tests/compile_time.py [runs] [gen_big.py args...]

Generates echo/examples/big.rs (via gen_big.py), warms the dependency cache, then reports for
check / debug build / release build:
  full - the example rebuilt from scratch (CARGO_INCREMENTAL=0)
  edit - incremental rebuild after editing a string literal inside one tree function
The generated file is deleted afterwards.
"""
import os, subprocess, sys, time

root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
runs = int(sys.argv[1]) if len(sys.argv) > 1 else 2
gen_args = sys.argv[2:]
big = os.path.join(root, "echo", "examples", "big.rs")
os.makedirs(os.path.dirname(big), exist_ok=True)


def gen(marker):
    src = subprocess.run(
        [sys.executable, os.path.join(root, "echo_tests", "gen_big.py"), *gen_args],
        capture_output=True, text=True, check=True,
    ).stdout
    # an edit in the middle of the first tree function
    src = src.replace('"Title 0"', f'"Title 0 {marker}"', 1)
    with open(big, "w") as f:
        f.write(src)
    return src


def run(cmd, env):
    r = subprocess.run(cmd, cwd=root, capture_output=True, text=True, env=env)
    if r.returncode != 0:
        print(r.stderr[-4000:])
        sys.exit(1)


try:
    src = gen(0)
    print(f"big.rs: {src.count(chr(10))} lines")
    cmds = [
        ["cargo", "check", "--example", "big"],
        ["cargo", "build", "--example", "big"],
        ["cargo", "build", "--release", "--example", "big"],
    ]
    full_env = dict(os.environ, CARGO_INCREMENTAL="0")
    inc_env = dict(os.environ)
    inc_env.pop("CARGO_INCREMENTAL", None)
    for cmd in cmds:
        run(cmd, inc_env)
    n = 1
    for cmd in cmds:
        res = []
        for label, env in (("full", full_env), ("edit", inc_env)):
            if label == "edit" and "--release" in cmd:
                continue  # release isn't incremental by default
            best = None
            for _ in range(runs):
                n += 1
                gen(n)
                t = time.perf_counter()
                run(cmd, env)
                dt = time.perf_counter() - t
                best = dt if best is None else min(best, dt)
            res.append(f"{label} {best:6.2f}s")
        print(f"{' '.join(cmd):<40} " + "   ".join(res))
finally:
    if os.path.exists(big):
        os.remove(big)
