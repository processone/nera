# Poolboy Worker Pool Integration Test

Tests that the [poolboy](https://github.com/devinus/poolboy) worker pool library
compiles and runs correctly with Nera.

**Baseline 2026-10-10: does not compile.** A fresh fetch of the configured 1.5.2
tag fails on `ets:match/2`, `ets:select/2`, `supervisor:start_child/2`,
`supervisor:terminate_child/2`, and `supervisor:which_children/1`.
The test is still optional in CI; it is a compatibility target, not evidence of
working Poolboy support. See [project status](../../../docs/PROJECT_STATUS.md).

## Running

```sh
cd tests/integration/poolboy
nera deps get        # fetch poolboy from GitHub
nera build           # compile poolboy + worker + test
./a.out              # run pool tests
```

## What it tests

- Pool startup with a registered name (`{local, my_pool}`)
- Basic checkout / `gen_server:call` / checkin cycle
- `poolboy:transaction/2` helper with a fun
- Saturation of the base pool plus overflow worker spawn
- `poolboy:status/1` reporting tuple
- Clean pool shutdown via `poolboy:stop/1`

## Why it matters

Poolboy is the canonical small-OTP exercise in the Erlang ecosystem. It pulls in
a stack of features Nera must support together:

- `gen_server` behaviour (init, handle_call, handle_cast, handle_info, terminate)
- `supervisor` behaviour (poolboy starts and supervises worker children)
- Poolboy's state is managed by its `gen_server`; the configured version does
  not require a `gen_fsm` / `gen_statem` behaviour
- ETS tables for monitor tracking
- Process monitors and link/trap_exit semantics
- Registered process names (`{local, Name}`)
- Cross-module `gen_server:call/2` and `gen_server:start_link/3`

If this test compiles and runs end-to-end, Nera has a credible OTP foundation —
the prerequisite for anything bigger (ranch, cowlib, cowboy).
