#!/usr/bin/env bash
#
# Build the serverless packet-parser demo and run it on this computer.
#
# Usage:
#   ./demo/serverless/build.sh            # build and run the local demo
#   ./demo/serverless/build.sh --package  # also write build/lambda.zip (Linux only)
#
# The local demo parses two sample packets compiled into the program and
# prints the results as Erlang terms. It reads no event.
#
# --package is an unqualified experiment, not a working Lambda function.
# Nobody has deployed it to AWS, and its bootstrap returns the same
# non-JSON text for every event. It only packages a Linux ELF executable
# and reports the architecture read from that file.
# See docs/SERVERLESS_ERLANG.md.
#
# Testing hook: with --package, NERA_SERVERLESS_PREBUILT_HANDLER=PATH packages
# that existing executable instead of building and running handler.erl. It
# lets the packaging step be checked on a computer that cannot build a Linux
# executable. The package is no more qualified than any other. The output
# path demo/serverless/build, which this script deletes first, and anything
# inside it are refused, however they are named.
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
OUT_DIR="$SCRIPT_DIR/build"

# Colors
CYAN='\033[0;36m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
RED='\033[0;31m'
BOLD='\033[1m'
RESET='\033[0m'

info()  { echo -e "${CYAN}==>${RESET} ${BOLD}$*${RESET}"; }
ok()    { echo -e "${GREEN}==>${RESET} ${BOLD}$*${RESET}"; }
warn()  { echo -e "${YELLOW}==>${RESET} ${BOLD}$*${RESET}"; }
err()   { echo -e "${RED}==>${RESET} ${BOLD}$*${RESET}" >&2; exit 1; }
usage() { echo "Usage: $0 [--package]"; }

PACKAGE=false
case "${1:-}" in
    "") ;;
    --package) PACKAGE=true ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
esac
[ $# -le 1 ] || { usage >&2; exit 2; }

PREBUILT="${NERA_SERVERLESS_PREBUILT_HANDLER:-}"
if [ -n "$PREBUILT" ] && ! $PACKAGE; then
    err "NERA_SERVERLESS_PREBUILT_HANDLER is only used with --package"
fi

# Print the Lambda architecture (x86_64 or arm64) of a 64-bit little-endian
# Linux ELF executable, read from its header. Fail for anything else, such
# as a Mach-O executable or an object file.
lambda_architecture() {
    local header
    header=$(od -An -tx1 -N20 -v "$1" | tr -d ' \n') || return 1
    # ELF magic, 64-bit class, little-endian data.
    [ "${header:0:12}" = "7f454c460201" ] || return 1
    # e_type: executable (2) or position-independent executable (3).
    case "${header:32:4}" in
        0200|0300) ;;
        *) return 1 ;;
    esac
    # e_machine: x86-64 (62) or AArch64 (183).
    case "${header:36:4}" in
        3e00) echo x86_64 ;;
        b700) echo arm64 ;;
        *) return 1 ;;
    esac
}

# Print the absolute path of an existing file with every symbolic link
# resolved, in the file name and in its folders.
physical_path() {
    local path="$1" link dir
    while [ -L "$path" ]; do
        link=$(readlink "$path") || return 1
        case "$link" in
            /*) path="$link" ;;
            *) path="$(dirname "$path")/$link" ;;
        esac
    done
    dir=$(cd "$(dirname "$path")" && pwd -P) || return 1
    echo "$dir/$(basename "$path")"
}

# --- Testing hook: check the prebuilt executable before deleting anything ----
# The output path is deleted below, so a prebuilt handler that is that path,
# lies inside it, or is reached through a link to either, would be destroyed
# before it is copied: refuse it instead. The test -ef comparison checks
# whether two names lead to the same file, following symbolic links, so it
# also catches other spellings on case-insensitive file systems.
if [ -n "$PREBUILT" ]; then
    warn "Testing hook: packaging $PREBUILT; handler.erl is not built or run"
    if [ ! -f "$PREBUILT" ] || [ ! -x "$PREBUILT" ]; then
        err "Not an executable file: $PREBUILT"
    fi
    PREBUILT=$(physical_path "$PREBUILT") || err "Cannot resolve $PREBUILT"
    if [ -e "$OUT_DIR" ]; then
        candidate="$PREBUILT"
        while :; do
            if [ "$candidate" -ef "$OUT_DIR" ]; then
                err "Refusing $PREBUILT: it is the output path $OUT_DIR
    or inside it, and this script deletes that path before packaging."
            fi
            [ "$candidate" != / ] || break
            candidate=$(dirname "$candidate")
        done
    fi
fi

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

if [ -n "$PREBUILT" ]; then
    cp "$PREBUILT" "$OUT_DIR/handler"
else
    # --- Step 1: Build nera compiler ------------------------------------------
    info "Building nera compiler (release)..."
    (cd "$PROJECT_DIR" && cargo build --release --locked --quiet)
    NERA="$PROJECT_DIR/target/release/nera"
    [ -x "$NERA" ] || err "nera binary not found at $NERA"

    # --- Step 2: Compile Erlang handler to native binary ----------------------
    # Build from this folder: nera build writes its temporary object files in
    # the current folder.
    info "Compiling handler.erl for this computer..."
    (cd "$SCRIPT_DIR" && "$NERA" build handler.erl -o "$OUT_DIR/handler" -e handler:start)
    [ -x "$OUT_DIR/handler" ] || err "Handler binary not produced"

    SIZE=$(stat -f%z "$OUT_DIR/handler" 2>/dev/null || stat -c%s "$OUT_DIR/handler")
    SIZE_MB=$(echo "scale=1; $SIZE / 1048576" | bc)
    info "Handler binary: ${SIZE_MB}MB"

    # --- Step 3: Run locally --------------------------------------------------
    info "Running handler locally..."
    OUTPUT=$("$OUT_DIR/handler" 2>&1) || err "Handler exited with an error:"$'\n'"$OUTPUT"
    [ -n "$OUTPUT" ] || err "Handler printed nothing"
    ok "Handler output:"
    echo "$OUTPUT" | head -5

    if ! $PACKAGE; then
        echo ""
        ok "Local demo built: $OUT_DIR/handler"
        echo "  No AWS Lambda package was made. --package writes an unqualified,"
        echo "  Linux-only experiment; see docs/SERVERLESS_ERLANG.md."
        exit 0
    fi
fi

# --- Step 4: Unqualified Lambda package (Linux ELF only) ---------------------
info "Checking the handler before packaging..."
if ! ARCH=$(lambda_architecture "$OUT_DIR/handler"); then
    err "Not packaged: $OUT_DIR/handler is not a 64-bit Linux executable
    (this computer: $(uname -s) $(uname -m)). AWS Lambda needs a Linux
    executable for x86_64 or arm64, and this script cannot build one here."
fi
command -v zip >/dev/null 2>&1 || err "zip is required for --package"

cp "$SCRIPT_DIR/bootstrap" "$OUT_DIR/bootstrap"
chmod +x "$OUT_DIR/bootstrap"
(cd "$OUT_DIR" && zip -q lambda.zip bootstrap handler)

ZIP_SIZE=$(stat -f%z "$OUT_DIR/lambda.zip" 2>/dev/null || stat -c%s "$OUT_DIR/lambda.zip")
ZIP_MB=$(echo "scale=1; $ZIP_SIZE / 1048576" | bc)

echo ""
warn "Unqualified Lambda package: $OUT_DIR/lambda.zip (${ZIP_MB}MB)"
echo "  Handler: Linux ELF executable, architecture $ARCH."
if [ -n "$PREBUILT" ]; then
    echo "  Testing hook: the handler is $PREBUILT, not built from handler.erl."
fi
echo "  Nobody has deployed this package to AWS Lambda. The handler ignores"
echo "  the event, and the bootstrap posts the same Erlang-term text, not"
echo "  JSON, for every invocation. Its library requirements were not checked"
echo "  against Amazon Linux 2023."
echo "  A function made from it would need --runtime provided.al2023 and"
echo "  --architectures $ARCH. See docs/SERVERLESS_ERLANG.md."
