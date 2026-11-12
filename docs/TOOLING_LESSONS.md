# Developer Tooling Lessons from Rebar4

Insights drawn from the [Rebar3-to-Rebar4 Kickstarter campaign](https://www.kickstarter.com/projects/peerstritzinger/rebar3-integrating-with-erlang-otp)
by Peer Stritzinger, and how they apply to nera.

## Context

Rebar4 is the next-generation Erlang build tool, funded via Kickstarter
(€13,420 raised from 158 backers, 152% of goal). Its central goal is to
integrate the build tool directly into Erlang/OTP so it ships with the
platform rather than being a separate install. Fred Hebert continues as
lead developer; the Erlang Ecosystem Foundation contributed €1,750.

## Lessons

### 1. Ship as Part of the Platform, Not Alongside It

Rebar4's core thesis: the build tool belongs *inside* OTP, not as a
separate download. Every extra install step is a friction point.

**For nera:** Our `find_stdlib_dir()` / `find_runtime_lib()` logic with
its 3-fallback chain (installed layout, workspace layout, CWD) is a
workaround for not being a single cohesive unit. The `scripts/dist.sh`
tarball is the right direction. A single static binary with embedded
stdlib would be the ideal end state.

### 2. Reduce External Dependencies

A concrete Rebar4 goal is replacing third-party deps with OTP-native
modules (e.g., replacing custom arg parsing with OTP's `argparse`).
Fewer deps means fewer breakage vectors and easier packaging.

**For nera:** Audit `Cargo.toml` files periodically. Each external crate
is a maintenance and supply-chain risk. For core functionality, prefer
self-contained implementations when the added code is small.

### 3. Editor/IDE Integration Matters Early

One of only two issues on the Rebar4 GitHub milestone is about making
dialyzer output parseable by Emacs (issue #1489). Error formatting is a
first-class concern, not an afterthought.

**For nera:** Compiler errors and warnings should follow the standard
`file:line:column: error/warning: message` format so editors can
jump-to-location. This is cheap to implement and pays outsized dividends
for adoption and daily usability.

### 4. stderr vs stdout Discipline

The other Rebar4 milestone issue (#1306) is about sending warnings to
stderr — a breaking change deferred to a major version. This matters for
tooling composability: piping, CI integration, editor parsing all depend
on clean stream separation.

**For nera:** Diagnostics (warnings, errors, progress info) should go to
stderr. Only compilation artifacts and program output should go to stdout.
Worth auditing the current state.

### 5. Multi-Language Ecosystem Thinking

Rebar4 explicitly targets "all BEAM languages" (Erlang, Elixir, Gleam),
not just Erlang. Designing for a single language when the ecosystem is
broader limits future reach.

**For nera:** Even though we only compile Erlang today, keeping the IR,
codegen, and runtime layers frontend-agnostic means a future LFE or
Gleam frontend could plug in without rearchitecting the pipeline.

### 6. Onboarding is a Tooling Problem

The Erlang forum discussion around the campaign highlighted that getting
started with Erlang is painful, and better build tooling directly
improves onboarding.

**For nera:** Potential improvements:
- A `nera new myproject` scaffolding command
- Sensible defaults when `-e` is omitted (try `main:start`, then
  `main:main`, then the single exported function if unambiguous)
- Clear, helpful error messages when files or modules are missing
- A `nera run file.erl` shorthand for compile-and-execute

### 7. Community Funding for Infrastructure Tools

The campaign demonstrates that developer tooling can be community-funded
(€85 average pledge), but corporate support is thin. Infrastructure
tools are chronically underfunded relative to their impact.

**For nera:** Keep the tool simple enough for a small team to maintain.
Design for contributor-friendliness: clear architecture, comprehensive
tests (548 unit + 182 end-to-end), and documented internals. A low
barrier to contribution is the best long-term sustainability strategy.

## Action Items

| Lesson | Concrete Action |
|---|---|
| Bundle with platform | Keep improving `dist.sh`; explore embedding stdlib in the binary |
| Reduce deps | Periodic `Cargo.toml` audit for unnecessary crates |
| Editor-friendly errors | Ensure `file:line:col: error/warning: message` format everywhere |
| stderr discipline | Route all diagnostics to stderr, only artifacts to stdout |
| Onboarding UX | Add `nera run` shorthand; improve defaults and error messages |
| Multi-frontend ready | Keep IR/codegen/runtime frontend-agnostic |
| Contributor-friendly | Maintain test coverage; document architecture decisions |

## References

- [Kickstarter Campaign](https://www.kickstarter.com/projects/peerstritzinger/rebar3-integrating-with-erlang-otp)
- [Erlang Forums Discussion](https://erlangforums.com/t/rebar4-kickstarter-building-on-top-of-rebar3-to-fully-integrate-with-erlang-otp-for-all-beam-languages/5072)
- [Rebar4 GitHub Milestone](https://github.com/erlang/rebar3/milestone/5)
- [Thinking Elixir Podcast 270](https://podcast.thinkingelixir.com/270)
