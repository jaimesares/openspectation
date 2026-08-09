#!/usr/bin/env bash
# Build openspectation in release mode and install its binaries into a local prefix.
set -euo pipefail

REPO_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
BIN_DIR="${BIN_DIR:-$HOME/.local/bin}"
BINARIES=(openspectation ospc)

command -v cargo >/dev/null 2>&1 || {
    echo "error: cargo not found in PATH" >&2
    exit 1
}

cargo build --release --manifest-path "$REPO_ROOT/Cargo.toml" "$@"

for bin in "${BINARIES[@]}"; do
    src="$TARGET_DIR/release/$bin"
    [ -x "$src" ] || {
        echo "error: missing build artifact: $src" >&2
        exit 1
    }
    install -Dm755 "$src" "$BIN_DIR/$bin"
    echo "installed $BIN_DIR/$bin"
done

case ":$PATH:" in
*":$BIN_DIR:"*) ;;
*) echo "warning: $BIN_DIR is not in PATH" >&2 ;;
esac

"$BIN_DIR/openspectation" --version
"$BIN_DIR/ospc" --version
