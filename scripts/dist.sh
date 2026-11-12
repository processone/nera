#!/usr/bin/env bash
#
# Build a distributable tarball for nera.
#
# Layout:
#   nera-<version>-<target>/
#     bin/nera
#     lib/nera/erlang/stdlib/*.erl
#
# The runtime is built before the compiler for embedding, and is also shipped
# at lib/libnera_runtime.a (lib/nera_runtime.lib on MSVC) for installed lookup.
#
# Usage:
#   ./scripts/dist.sh                          # build for current host
#   ./scripts/dist.sh --version 0.2.0          # override version
#   ./scripts/dist.sh --target x86_64-apple-darwin  # cross-compile

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

# Defaults
VERSION="0.1.0"
TARGET=""

# Parse arguments
while [[ $# -gt 0 ]]; do
    case "$1" in
        --version)
            VERSION="$2"
            shift 2
            ;;
        --target)
            TARGET="$2"
            shift 2
            ;;
        *)
            echo "Unknown option: $1" >&2
            echo "Usage: $0 [--version <ver>] [--target <triple>]" >&2
            exit 1
            ;;
    esac
done

# Use Rust's canonical host triple (macOS uname reports arm64, Rust aarch64).
HOST_TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"
if [[ -z "$HOST_TRIPLE" ]]; then
    echo "error: cannot determine Rust host triple" >&2
    exit 1
fi

# Default target to host
if [[ -z "$TARGET" ]]; then
    TARGET="$HOST_TRIPLE"
fi

IS_CROSS=false
if [[ "$TARGET" != "$HOST_TRIPLE" ]]; then
    IS_CROSS=true
fi

# Detect Windows target
IS_WINDOWS=false
case "$TARGET" in
    *-windows-*) IS_WINDOWS=true ;;
esac

# Binary and library names
if $IS_WINDOWS; then
    BIN_NAME="nera.exe"
    case "$TARGET" in
        *-msvc) RUNTIME_LIB_NAME="nera_runtime.lib" ;;
        *) RUNTIME_LIB_NAME="libnera_runtime.a" ;;
    esac
else
    BIN_NAME="nera"
    RUNTIME_LIB_NAME="libnera_runtime.a"
fi

DIST_NAME="nera-${VERSION}-${TARGET}"
DIST_DIR="${ROOT_DIR}/dist/${DIST_NAME}"
if $IS_WINDOWS; then
    ARCHIVE="${ROOT_DIR}/dist/${DIST_NAME}.zip"
else
    ARCHIVE="${ROOT_DIR}/dist/${DIST_NAME}.tar.gz"
fi

echo "Building nera ${VERSION} for ${TARGET}..."
if $IS_CROSS; then
    echo "Cross-compiling from ${HOST_TRIPLE}"
fi

# A target-labelled archive must contain a compiler AND runtime for that target.
# Build the runtime first: nera's build script embeds the completed static library.
cd "$ROOT_DIR"
TARGET_DIR="${CARGO_TARGET_DIR:-${ROOT_DIR}/target}"
case "$TARGET_DIR" in
    /*) ;;
    *) TARGET_DIR="${ROOT_DIR}/${TARGET_DIR}" ;;
esac
BUILD_ARGS=(--release --locked)
ARTIFACT_DIR="${TARGET_DIR}/release"
if $IS_CROSS; then
    BUILD_ARGS+=(--target "$TARGET")
    ARTIFACT_DIR="${TARGET_DIR}/${TARGET}/release"
fi
cargo build "${BUILD_ARGS[@]}" -p nera-runtime
cargo build "${BUILD_ARGS[@]}" -p nera
NERA_BIN="${ARTIFACT_DIR}/${BIN_NAME}"
RUNTIME_LIB="${ARTIFACT_DIR}/${RUNTIME_LIB_NAME}"

STDLIB_DIR="${ROOT_DIR}/erlang/stdlib"

# Verify artifacts exist
if [[ ! -f "$NERA_BIN" ]]; then
    echo "error: nera binary not found at $NERA_BIN" >&2
    exit 1
fi

if [[ -n "$RUNTIME_LIB" && ! -f "$RUNTIME_LIB" ]]; then
    echo "error: runtime library not found at $RUNTIME_LIB" >&2
    exit 1
fi

if [[ ! -d "$STDLIB_DIR" ]]; then
    echo "error: stdlib directory not found at $STDLIB_DIR" >&2
    exit 1
fi

# Create distribution directory
rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR/bin"
mkdir -p "$DIST_DIR/lib/nera/erlang/stdlib"

# Copy artifacts
cp "$NERA_BIN" "$DIST_DIR/bin/"
cp "$STDLIB_DIR"/*.erl "$DIST_DIR/lib/nera/erlang/stdlib/"

# Match the compiler's installed runtime lookup path.
cp "$RUNTIME_LIB" "$DIST_DIR/lib/"

# Strip the binary (reduce size) — skip on Windows
if ! $IS_WINDOWS && command -v strip &>/dev/null; then
    strip "$DIST_DIR/bin/${BIN_NAME}" 2>/dev/null || true
fi

# Replace an old archive rather than updating it and retaining stale entries.
rm -f "$ARCHIVE"
# Create archive
cd "${ROOT_DIR}/dist"
if $IS_WINDOWS; then
    if command -v 7z &>/dev/null; then
        7z a -tzip "$ARCHIVE" "$DIST_NAME"
    elif command -v powershell &>/dev/null; then
        powershell -Command "Compress-Archive -Path '$DIST_NAME' -DestinationPath '$ARCHIVE'"
    else
        # Fallback: use zip if available, otherwise tar
        zip -r "$ARCHIVE" "$DIST_NAME"
    fi
else
    COPYFILE_DISABLE=1 tar czf "$ARCHIVE" "$DIST_NAME"
fi

# Summary
echo ""
echo "Distribution created:"
echo "  $ARCHIVE"
echo ""
echo "Contents:"
find "$DIST_DIR" -type f | sort | while read -r f; do
    SIZE=$(wc -c < "$f" | tr -d ' ')
    REL="${f#$DIST_DIR/}"
    printf "  %-40s %s\n" "$REL" "$(numfmt --to=iec $SIZE 2>/dev/null || echo "${SIZE} bytes")"
done
echo ""
if $IS_WINDOWS; then
    echo "Install:"
    echo "  Expand-Archive ${DIST_NAME}.zip -DestinationPath ."
    echo "  \$env:PATH = \"\$(Get-Location)\\${DIST_NAME}\\bin;\$env:PATH\""
else
    echo "Install:"
    echo "  tar xzf ${DIST_NAME}.tar.gz"
    echo "  export PATH=\"\$(pwd)/${DIST_NAME}/bin:\$PATH\""
fi
