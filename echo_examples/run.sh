#!/usr/bin/env bash
# Builds the examples and opens them in Godot.
#
#   echo_examples/run.sh              open the example gallery
#   echo_examples/run.sh --release
#   echo_examples/run.sh --smoke      headless: click through every example, fail on a panic
#
# Godot binary: $GODOT4_BIN, else ./godot at the workspace root.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/.." && pwd)"
godot="${GODOT4_BIN:-$root/godot}"
target="${CARGO_TARGET_DIR:-$root/target}"

profile=debug
cargo_flags=()
smoke=false
for a in "$@"; do
    case "$a" in
        --release) profile=release; cargo_flags+=(--release) ;;
        --smoke) smoke=true ;;
        *) echo "unknown argument: $a" >&2; exit 2 ;;
    esac
done

cargo build --quiet -p echo_examples "${cargo_flags[@]}" --manifest-path "$root/Cargo.toml"
mkdir -p "$here/godot/bin" "$here/godot/.godot"
cp "$target/$profile/libecho_examples.so" "$here/godot/bin/"
echo 'res://echo_examples.gdextension' > "$here/godot/.godot/extension_list.cfg"

if $smoke; then
    log="$(mktemp)"
    "$godot" --headless --path "$here/godot" -- --smoke 2>&1 | tee "$log"
    if grep -qiE 'panic|SCRIPT ERROR|^ERROR' "$log" || ! grep -q 'smoke test passed' "$log"; then
        echo "smoke test FAILED" >&2
        exit 1
    fi
else
    exec "$godot" --path "$here/godot"
fi
