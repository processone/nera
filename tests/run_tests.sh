#!/usr/bin/env bash
#
# End-to-end test runner for nera
#
# Each .erl file in this directory should define a module that exports test/0.
# test/0 must return 'ok' on success; any crash (badmatch, etc.) signals failure.
#
# Usage:
#   ./tests/run_tests.sh              # run all tests
#   ./tests/run_tests.sh guard_*      # run tests matching a pattern
#   ./tests/run_tests.sh --list       # list available tests
#
# Exit code: number of failed tests (0 = all passed)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
NERA="$PROJECT_DIR/target/release/nera"
TEST_DIR="$SCRIPT_DIR"
TMP_DIR="${TMPDIR:-/tmp}/nera_tests_$$"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
RESET='\033[0m'

cleanup() {
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT

mkdir -p "$TMP_DIR"

# Check that nera is built
if [ ! -x "$NERA" ]; then
    echo -e "${RED}Error: nera not found at $NERA${RESET}"
    echo "Run 'cargo build --release' first."
    exit 1
fi

# --list mode
if [ "${1:-}" = "--list" ]; then
    echo "Available tests:"
    for f in "$TEST_DIR"/*.erl; do
        [ -f "$f" ] || continue
        basename "$f" .erl
    done
    exit 0
fi

# Collect test files
test_files=()
if [ $# -gt 0 ]; then
    for pattern in "$@"; do
        for f in "$TEST_DIR"/$pattern.erl; do
            [ -f "$f" ] && test_files+=("$f")
        done
    done
else
    for f in "$TEST_DIR"/*.erl; do
        [ -f "$f" ] || continue
        # Skip files starting with _ (helpers)
        base="$(basename "$f")"
        [[ "$base" == _* ]] && continue
        test_files+=("$f")
    done
fi

if [ ${#test_files[@]} -eq 0 ]; then
    echo "No test files found."
    exit 0
fi

total=${#test_files[@]}
passed=0
failed=0
compile_failed=0
skipped=0

echo -e "${BOLD}Running $total end-to-end tests for nera${RESET}"
echo "─────────────────────────────────────────────"

for test_file in "${test_files[@]}"; do
    test_name="$(basename "$test_file" .erl)"
    module_name="$test_name"
    binary="$TMP_DIR/$test_name"

    # Check for skip marker: first line comment containing "SKIP:"
    if head -1 "$test_file" | grep -q '%% SKIP:'; then
        reason="$(head -1 "$test_file" | sed 's/.*SKIP: *//')"
        echo -e "  ${YELLOW}SKIP${RESET}  $test_name  ($reason)"
        skipped=$((skipped + 1))
        continue
    fi

    # Read expected output if present (comment: %% EXPECT: <text>)
    expected=""
    if grep -q '%% EXPECT:' "$test_file"; then
        expected="$(grep '%% EXPECT:' "$test_file" | head -1 | sed 's/.*EXPECT: *//')"
    fi

    # Check for expected compile error (comment: %% EXPECT_COMPILE_ERROR: <text>)
    expected_compile_error=""
    if grep -q '%% EXPECT_COMPILE_ERROR:' "$test_file"; then
        expected_compile_error="$(grep '%% EXPECT_COMPILE_ERROR:' "$test_file" | head -1 | sed 's/.*EXPECT_COMPILE_ERROR: *//')"
    fi

    # Collect companion modules (multi-module tests)
    # Convention: %% MODULES: _helper1.erl _helper2.erl
    extra_modules=()
    if grep -q '%% MODULES:' "$test_file"; then
        module_list="$(grep '%% MODULES:' "$test_file" | head -1 | sed 's/.*MODULES: *//')"
        for m in $module_list; do
            extra_modules+=("$TEST_DIR/$m")
        done
    fi

    # Compile
    compile_output=$("$NERA" build "$test_file" ${extra_modules[@]+"${extra_modules[@]}"} -o "$binary" -e "$module_name:test" 2>&1) || {
        if [ -n "$expected_compile_error" ]; then
            # We expected a compile error — check the output matches
            if echo "$compile_output" | grep -qF "$expected_compile_error"; then
                echo -e "  ${GREEN}PASS${RESET}  $test_name  (expected compile error)"
                passed=$((passed + 1))
            else
                echo -e "  ${RED}FAIL${RESET}  $test_name  (compile error mismatch)"
                echo "        Expected: $expected_compile_error"
                echo "        $compile_output" | head -5
                compile_failed=$((compile_failed + 1))
                failed=$((failed + 1))
            fi
        else
            echo -e "  ${RED}FAIL${RESET}  $test_name  (compile error)"
            echo "        $compile_output" | head -5
            compile_failed=$((compile_failed + 1))
            failed=$((failed + 1))
        fi
        continue
    }

    # If we expected a compile error but compilation succeeded, that's a failure
    if [ -n "$expected_compile_error" ]; then
        echo -e "  ${RED}FAIL${RESET}  $test_name  (expected compile error but compilation succeeded)"
        failed=$((failed + 1))
        continue
    fi

    # Run with timeout (kill-based fallback works on all platforms)
    TEST_TIMEOUT=10
    run_output=""
    exit_code=0
    timed_out=false

    if command -v gtimeout >/dev/null 2>&1; then
        run_output=$(gtimeout "$TEST_TIMEOUT" "$binary" 2>&1) || exit_code=$?
        [ $exit_code -eq 124 ] && timed_out=true
    elif command -v timeout >/dev/null 2>&1; then
        run_output=$(timeout "$TEST_TIMEOUT" "$binary" 2>&1) || exit_code=$?
        [ $exit_code -eq 124 ] && timed_out=true
    else
        # Kill-based timeout for macOS (no coreutils timeout)
        outfile="$TMP_DIR/${test_name}.out"
        "$binary" >"$outfile" 2>&1 &
        test_pid=$!
        ( sleep "$TEST_TIMEOUT" && kill -9 "$test_pid" 2>/dev/null ) &
        watchdog_pid=$!
        wait "$test_pid" 2>/dev/null || exit_code=$?
        kill "$watchdog_pid" 2>/dev/null || true
        wait "$watchdog_pid" 2>/dev/null || true
        run_output=$(cat "$outfile" 2>/dev/null || true)
        rm -f "$outfile"
        # SIGKILL (137) from watchdog means timeout
        [ $exit_code -eq 137 ] && timed_out=true
    fi

    if $timed_out; then
        echo -e "  ${RED}HANG${RESET}  $test_name  (killed after ${TEST_TIMEOUT}s)"
        # Clean up any lingering worker threads
        pkill -9 -f "$binary" 2>/dev/null || true
        failed=$((failed + 1))
        continue
    elif [ $exit_code -ne 0 ]; then
        echo -e "  ${RED}FAIL${RESET}  $test_name  (exit=$exit_code)"
        echo "        $(echo "$run_output" | head -3)"
        failed=$((failed + 1))
        continue
    fi

    # Check expected output if specified
    if [ -n "$expected" ]; then
        if echo "$run_output" | grep -qF "$expected"; then
            echo -e "  ${GREEN}PASS${RESET}  $test_name"
            passed=$((passed + 1))
        else
            echo -e "  ${RED}FAIL${RESET}  $test_name  (output mismatch)"
            echo "        Expected: $expected"
            echo "        Got:      $(echo "$run_output" | head -1)"
            failed=$((failed + 1))
        fi
    else
        # No expected output — just check that it didn't crash
        echo -e "  ${GREEN}PASS${RESET}  $test_name"
        passed=$((passed + 1))
    fi
done

echo "─────────────────────────────────────────────"
echo -e "${BOLD}Results:${RESET} $passed passed, $failed failed ($compile_failed compile errors), $skipped skipped / $total total"

if [ $failed -gt 0 ]; then
    echo -e "${RED}${BOLD}SOME TESTS FAILED${RESET}"
else
    echo -e "${GREEN}${BOLD}ALL TESTS PASSED${RESET}"
fi

exit $failed
