# cowlib_demo

A small project that depends on [Cowlib](https://github.com/ninenines/cowlib)
2.22.0 from hex.pm and runs it natively on Nera. `cowlib_demo:start/0` makes
85 calls across all 25 Cowlib modules (cookies, dates, URIs, HTTP headers,
HTTP/2 and HTTP/3 framing, HPACK and QPACK, multipart, WebSocket masking, and
others) and prints one line per call.

Run it:

```sh
cd examples/cowlib_demo
nera deps get
nera build
./cowlib_demo
```

The last line is `SUMMARY 85 checks, 85 returned, 0 raised`.

The same sources run on OTP. `rebar.config` fetches Cowlib for rebar3:

```sh
rebar3 compile
mkdir -p ebin && erlc -o ebin src/cowlib_demo.erl
erl -noshell -pa ebin _build/default/lib/cowlib/ebin \
    -eval 'cowlib_demo:start(), halt().'
```

To compare the two runs, call `cowlib_demo:start_canonical()`, which prints map
entries sorted and atoms quoted, so that only a difference in value shows up.
`scripts/test-cowlib-demo.py` builds both and checks that the sorted outputs
match, except for four known lines. `benchmarks/cowlib_bench.sh` times the same
program on both runtimes (on demand; it is not a correctness check).

## Result (Nera main 3b2abf2, macOS arm64, OTP 29.1.2)

- `nera check`: 0 errors and 57 warnings (non-tail-call lints,
  test-only macros, unused-type false positives).
- All 85 calls return natively with no exception, and the output is stable over
  10 runs.
- Against OTP 29, 75 lines are byte-identical. 8 print the same value
  differently (6 maps with atom keys listed in another key order, 2
  `'HTTP/1.1'` atoms printed without quotes, NERA-18). 2 differ in byte order
  only because Cowlib walks an atom-keyed map (`Set-Cookie` attribute order,
  HTTP/2 pseudo-header order); sorted, they are equivalent. The HTTP/2
  SETTINGS frame is byte-identical and re-parses to the original map on both
  runtimes. In the sorted comparison, 87 of 91 lines are identical; the 4 left
  are those 2 orders and 2 atom-quoting probes.

Speed, measured on an Apple M4 (shared machine, so treat as indicative; rerun
`benchmarks/cowlib_bench.sh`):

| | Nera | OTP 29.1.2 |
|---|---|---|
| Whole program, median wall time | 6.6 ms | 128 ms |
| Warm loop, one pass over 84 checks (HTTP/2 machine left out) | 436 us | 102 us |
| Peak memory footprint | 5.7 MB | 78 MB |

Nera wins a single run because it starts in milliseconds; OTP's JIT is about 4
times faster once warm, with short binary parsers the widest gap. The HTTP/2
machine check is left out of the loop because each native timer holds an OS
thread until it fires (NERA-88).

Not covered: Cowlib's own upstream test suites (see
`tests/integration/cowlib`), Linux, and Cowboy.
