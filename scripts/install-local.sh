#!/usr/bin/env bash
#
# Build and install nera locally.
#
# Layout (default prefix: ~/.local):
#   ~/.local/bin/nera
#
# The runtime library is embedded in the nera binary, so only the
# binary needs to be installed.
#
# Usage:
#   ./scripts/install-local.sh              # install to ~/.local
#   ./scripts/install-local.sh --prefix ~/nera   # custom prefix
#   ./scripts/install-local.sh --debug       # use debug build

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

# Defaults
PREFIX="$HOME/.local"
PROFILE="release"
CARGO_FLAG="--release"

# Parse arguments
while [[ $# -gt 0 ]]; do
    case "$1" in
        --prefix)
            PREFIX="$2"
            shift 2
            ;;
        --debug)
            PROFILE="debug"
            CARGO_FLAG=""
            shift
            ;;
        -h|--help)
            echo "Usage: $0 [--prefix <dir>] [--debug]"
            echo ""
            echo "Build and install nera to a local prefix."
            echo ""
            echo "Options:"
            echo "  --prefix <dir>   Install prefix (default: ~/.local)"
            echo "  --debug          Use debug build instead of release"
            echo ""
            echo "After install, make sure $PREFIX/bin is in your PATH:"
            echo "  export PATH=\"$PREFIX/bin:\$PATH\""
            exit 0
            ;;
        *)
            echo "Unknown option: $1" >&2
            echo "Usage: $0 [--prefix <dir>] [--debug]" >&2
            exit 1
            ;;
    esac
done

echo "Building nera ($PROFILE)..."
cd "$ROOT_DIR"
cargo build $CARGO_FLAG

# Source paths
NERA_BIN="${ROOT_DIR}/target/${PROFILE}/nera"
if [[ ! -f "$NERA_BIN" ]]; then
    echo "error: not found: $NERA_BIN" >&2
    exit 1
fi

# Install
mkdir -p "$PREFIX/bin"
cp "$NERA_BIN" "$PREFIX/bin/nera"

# Strip release binary
if [[ "$PROFILE" == "release" ]] && command -v strip &>/dev/null; then
    strip "$PREFIX/bin/nera" 2>/dev/null || true
fi

echo ""
echo "Installed nera to $PREFIX"
echo "  $PREFIX/bin/nera"
echo ""

# Check PATH
if ! echo "$PATH" | tr ':' '\n' | grep -qx "$PREFIX/bin"; then
    echo "Add to your shell profile (~/.zshrc or ~/.bashrc):"
    echo "  export PATH=\"$PREFIX/bin:\$PATH\""
    echo ""
fi

echo "Test: nera --help"
