#!/usr/bin/env bash
# Builds the test extension and runs it in headless Godot.
#
#   echo_tests/run.sh                 run all tests (debug build)
#   echo_tests/run.sh memo            run tests whose name contains "memo"
#   echo_tests/run.sh --release --bench [filter]
#
# Godot binary: $GODOT4_BIN, else ./godot at the workspace root.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/.." && pwd)"
godot="${GODOT4_BIN:-$root/godot}"
target="${CARGO_TARGET_DIR:-$root/target}"

profile=debug
cargo_flags=()
args=()
for a in "$@"; do
    case "$a" in
        --release) profile=release; cargo_flags+=(--release) ;;
        *) args+=("$a") ;;
    esac
done

cargo build --quiet -p echo_tests "${cargo_flags[@]}" --manifest-path "$root/Cargo.toml"
mkdir -p "$here/godot/bin" "$here/godot/.godot"
cp "$target/$profile/libecho_tests.so" "$here/godot/bin/"
echo 'res://echo_tests.gdextension' > "$here/godot/.godot/extension_list.cfg"
exec "$godot" --headless --path "$here/godot" -- "${args[@]}"
