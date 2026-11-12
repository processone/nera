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

## Result (Nera main 22545e6, macOS arm64, OTP 29.1.2)

- `nera check`: 0 errors and 57 warnings (non-tail-call lints,
  test-only macros, unused-type false positives).
- All 85 calls return natively with no exception, and the output is stable over
  10 runs.
- Against OTP 29, 74 lines are byte-identical. 8 print the same value
  differently (6 map key orders, 2 `'HTTP/1.1'` atoms printed without quotes).
  3 differ in byte order only because Cowlib iterates a map (`Set-Cookie`
  attribute order, HTTP/2 pseudo-header order, SETTINGS frame order); sorted,
  they are equivalent, and the SETTINGS frame re-parses to the original map on
  both runtimes.

Nera appears to iterate small maps in insertion order, while OTP orders them by
key. Not covered: Cowlib's own upstream test suites (see
`tests/integration/cowlib`), Linux, and Cowboy.
