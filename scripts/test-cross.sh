#!/usr/bin/env bash
# Compile a standalone program outside the checkout and inspect its object.
# Usage: scripts/test-cross.sh /absolute/path/to/nera TARGET
set -euo pipefail

COMPILER="${1:?compiler path required}"
TARGET="${2:?target triple required}"
case "$COMPILER" in
    /*) ;;
    *) COMPILER="$(pwd)/$COMPILER" ;;
esac
case "$TARGET" in
    x86_64-apple-darwin) EXPECTED='Mach-O 64-bit.*x86_64' ;;
    aarch64-unknown-linux-gnu) EXPECTED='ELF 64-bit.*ARM aarch64' ;;
    *) echo "unsupported smoke-test target: $TARGET" >&2; exit 2 ;;
esac

WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/nera-cross-XXXXXX")"
trap 'rm -rf "$WORK_DIR"' EXIT
cd "$WORK_DIR"
cat > hello.erl <<'ERLANG'
-module(hello).
-export([start/0]).
start() ->
    <<"f">> = base64:decode(<<"Zg==">>, #{}),
    <<"f">> = base64:decode("Zg==", #{}),
    io:format("Hello from nera!~n").
ERLANG

"$COMPILER" build hello.erl --target "$TARGET" --emit-obj -v
test -s hello.o
file hello.o | tee object-format.txt
grep -Eq "$EXPECTED" object-format.txt
