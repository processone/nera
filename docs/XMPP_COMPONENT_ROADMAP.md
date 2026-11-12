# Nera positioning and XMPP component roadmap

**Direction recorded: 2026-10-14. Status: planned; no demonstrator milestone is
validated by this document.**

Nera aims to reuse Erlang code to build concurrent services, check their
contracts during compilation, and distribute them as standalone native
executables. The first demonstration of this combination will be an external
XMPP component with a statically checked Erlang application core.

This roadmap defines the component's scope, implementation priorities and
acceptance criteria. [Project status](PROJECT_STATUS.md) and the
[compatibility register](../COMPATIBILITY.md) remain the sources for observed
results and release readiness. This roadmap does not waive their existing
correctness or release gates.

## Positioning

The intended users are Erlang developers and teams extending existing messaging
systems. They should be able to retain useful source code and tests, add checks
incrementally, and deploy a service without installing Erlang/OTP on its host.

| Benefit | What the demonstrator must establish |
| --- | --- |
| Reuse existing Erlang | Compile identifiable upstream code with a documented, limited set of adaptations. |
| Check contracts before deployment | Reject representative type errors in the selected Erlang modules during the build. |
| Retain Erlang concurrency | Exercise processes, supervision and explicit recovery under connection and worker failures. |
| Deploy a native service | Run the packaged executable on a clean target without the compiler, source checkout or Erlang installation. |

The combination is the proposition. Native output alone does not establish a
memory or performance advantage over Go, Rust or BEAM. Static checks strengthen
the development feedback loop, including LLM-assisted edits; protocol tests and
runtime validation still establish behavior at external boundaries.

Linux services are the first deployment target. Linux ARM64 appliances and
embedded agents are a follow-on qualification target. Microcontrollers and
hard real-time workloads require separate runtime and platform work.

## Demonstrator scope

Build an alert-routing component at a configurable domain such as
`alerts.example.org`. An authorized sender submits a structured XMPP message.
The component validates the event, applies Erlang routing rules, and sends
notifications from its own component domain to configured recipients. Include
service discovery and explicit errors for unsupported requests.

The component connects to an existing server using
[XEP-0114](https://xmpp.org/extensions/xep-0114.html). Its process, executable,
configuration and update cycle are independent; its server attachment provides
XMPP routing and federation. The initial target is ejabberd, followed by a
separately validated Prosody configuration.

The first implementation uses fixed configuration and bounded in-memory work
queues. It does not acknowledge durable acceptance or promise delivery after
process or host failure. Define overflow, disconnect and retry behavior before
implementation, and expose failures to the operator. A durable spool would be
a later feature with its own persistence and delivery tests.

Use the existing `processone/xmpp` codecs and utilities where practical. Extract
ejabberd logic only where it provides useful behavior with a manageable
dependency boundary. Record the actual source reused; a new service that merely
resembles ejabberd does not satisfy the reuse objective.

### Architecture and verification boundary

```text
XMPP server
    |
    | XEP-0114 component connection
    v
Connection process and supervision
    |
    v
fxml / fxml_stream API -> native Nera adapter -> quick-xml
    |
    v
Reused xmpp codecs and explicit input validation
    |
    v
Typed Erlang event -> checked routing rules -> typed actions
    |
    v
XMPP encoding and bounded outbound work
```

`quick-xml` is the proposed parser backend. Compatibility with the selected
`fast_xml` API and fragmented stream behavior must be demonstrated in P2 before
adopting it for the component. The integration targets the Erlang API and term
representations; BEAM NIF binary compatibility is not required.

The first static claim is **a statically checked Erlang application core**.
Publish the exact module/function scope and distinguish:

- Erlang implementations whose bodies and calls are checked;
- dependencies with declared interfaces whose bodies are not yet checked;
- trusted native adapters and runtime functions;
- external values validated at runtime before entering the checked core.

A declared dependency spec is an assumption, not proof of its implementation.
Network inputs, configuration and native results must cross documented validation
boundaries. Unknown types or unsupported constructs must not silently acquire
the status of checked code.

## Starting point

The current syntax representation records names, arities and source spans for
`-type`, `-opaque`, `-spec` and `-callback`; it does not yet represent their full
type expressions for enforcement. The LSP has heuristic type inference used
for suggestions, not a checker of implementation contracts. See
[syntax definitions](../crates/nera-syntax/src/ast.rs) and
[LSP inference](../crates/nera/src/lsp/type_infer.rs).

The [compatibility register](../COMPATIBILITY.md) lists ejabberd as untested.
Existing TCP and runtime results are useful foundations but do not validate
this component, its XML adapter, its TLS integration or its target hardware.
P0 must establish their exact status for the selected revisions.

## Milestones and dependencies

All milestones below are planned. P0 fixes a reproducible scope. P1 and P2 can
then proceed independently. P3 validates source reuse; P4 combines it with the
checked application core. P5 qualifies the complete executable on each target.
Build an early vertical slice as soon as P1 supports the first event handler
and P2 supports a complete stanza; do not postpone static checking until after
the entire service works.

| Priority | Deliverable | Exit evidence |
| --- | --- | --- |
| P0 | Scope and BEAM reference | Pinned dependencies, behavior contract and runnable reference corpus. |
| P1 | Enforced static contracts | Accepted fixtures, rejected negative fixtures and an explicit checking boundary. |
| P2 | Native XML compatibility | Differential XML results, fragmentation coverage and resource-limit results. |
| P3 | Reused Erlang XMPP code | Compiled upstream subset, adaptation inventory and BEAM/Nera equivalence results. |
| P4 | Working checked component | ejabberd/Prosody exchanges, failure recovery and strict application build. |
| P5 | Reproducible native deployment | Clean-host execution, target-specific endurance results and measurements. |

### P0 Define the behavior and reference

**Work:** select and pin Nera, OTP, `xmpp`, `fast_xml`, the proposed Rust parser
and server versions. Inventory the required functions and their transitive
dependencies, including XML, JID normalization, hashing, networking and any OTP
behaviors. Keep the first event schema and routing rules small.

**Validation criteria:**

- A BEAM reference runs the selected Erlang logic against fixtures containing
  valid alerts, malformed payloads, unknown event variants, unauthorized senders
  and unsupported requests, with explicit expected actions or errors.
- The connection contract specifies component identity, authentication,
  permitted senders and recipients, reconnection, queue overflow and failure
  reporting. Routing prevents notification loops.
- A source manifest identifies reused files/functions, upstream commits,
  generated code, local adaptations and licensing/provenance requirements.
- A gap inventory distinguishes missing APIs, unknown compatibility and known
  runtime defects. Every dependency needed by a later milestone is accounted for.
- Workload, queue limits, stanza limits, timeouts and resource budgets are fixed
  before acceptance runs. Threshold changes require an explanation and a rerun;
  failed cases cannot disappear through a reduced denominator.

**Evidence:** reference commands, outputs, fixture inventory, source/dependency
manifest and the selected acceptance budgets.

### P1 Enforce the selected Erlang type contracts

**Work:** parse full type expressions with source locations and preserve them
through the checking pipeline. Support the primitives, literal atoms, unions,
tuples, records, lists, typed maps and named local/remote types required by the
component. Use declared function contracts, local inference and refinement from
patterns/guards to check bodies, returns and calls across module boundaries.

Provide a strict checking mode for explicitly selected modules. Unsupported
constructs or unresolved types in that scope are blocking diagnostics. Outside
it, require an explicit boundary and report the assumptions. The mode and CLI
spelling are implementation work, not existing commands.

**Validation criteria:**

- Correct fixtures compile and run, including valid unions, optional map fields,
  guard refinements and calls through declared dependency interfaces.
- Negative fixtures fail with a nonzero build result for wrong argument types,
  return/spec mismatches, missing required map fields, incorrect record fields
  and unhandled cases in the supported closed unions.
- Diagnostics identify the source location, expected and observed types, and
  the relevant contract. A rejected build must not leave a newly produced
  executable that appears successful.
- Unsupported syntax, unresolved remote types and unchecked dynamic calls in
  strict scope are reported explicitly. They cannot pass by becoming `term()`.
- Input validators are checked as Erlang functions and tested with invalid
  values; annotations or unchecked casts cannot substitute for validation.
- The build records the checked scope, unverified dependencies and native
  interface assumptions. Checking a spec without checking its body does not
  count as a verified implementation.

**Evidence:** positive/negative fixture results, diagnostic snapshots and the
checking-boundary report. This milestone establishes the supported checks, not
a general proof of absence of runtime errors or a type system for every Erlang
program. Typed process mailboxes and whole-program inference are later work.

### P2 Implement the native XML boundary

**Work:** retain reusable Erlang helpers and implement the required `fxml` and
`fxml_stream` entry points against the native backend. Start with element parsing
and serialization, then cover stream creation, incremental parsing, reset,
closure and callback delivery. Include additional options only when the pinned
call graph requires them.

**Validation criteria:**

- Differential tests against pinned `fast_xml` cover element structure,
  attributes, text/CDATA, significant whitespace, namespaces, escaping, Unicode,
  malformed input and the supported error contract.
- Short fixtures are split at every byte boundary and fed one byte at a time;
  larger fixtures use reproducible random partitions. Include splits inside
  UTF-8 characters, entities, attributes and closing tags.
- An open stream emits completed stanzas without waiting for the root to close.
  Multiple stanzas in one fragment preserve event order. Reset and close release
  parser state; changing the callback is tested if used.
- Stanza size, nesting, element count and parser-buffer limits have bounded,
  observable failure behavior. A concurrent heartbeat test checks that parsing
  work respects the scheduling budget fixed in P0.
- Parser resources are reclaimed after repeated create/reset/close cycles and
  owning-process failures. Malformed inputs do not crash the host process.
- Compare observable terms and events. Any normalization, error mapping or
  intentional incompatibility is explicit and reviewed; acceptance differences
  cannot be hidden by normalizing the outputs.

**Evidence:** differential results, partition seeds, resource and scheduling
measurements, plus any fuzz campaign duration, inputs and findings. The presence
of a fuzz harness alone is not a successful campaign.

### P3 Compile and validate the reused XMPP subset

**Work:** compile the pinned codec and utility subset required for the component.
Keep upstream Erlang sources where possible; make adapters explicit. JID and
namespace handling must follow the selected upstream behavior rather than an
ad hoc string implementation.

**Validation criteria:**

- The manifest accounts for every required dependency and identifies changed,
  unchanged, generated and replaced source. No placeholder implementation can
  satisfy a required behavior.
- Selected upstream assertion bodies and the P0 corpus execute on both BEAM and
  Nera. Report passed, failed, blocked and skipped cases separately.
- Both engines agree on decoded structures, validation outcomes, routing inputs
  and encoded output semantics for valid and invalid inputs. Add independent
  expected protocol fixtures so agreement alone is not the only oracle.
- Required error paths and repeated execution pass in the native process,
  including memory reclamation across many decode/encode cycles.
- P1's boundary report covers the compiled subset: checked code and code used
  only through declared contracts remain distinguishable.

**Evidence:** dependency/adaptation manifest, differential outputs, upstream
test results and the exact checking scope. Add a scoped compatibility entry
only after the corresponding execution evidence exists.

### P4 Integrate the supervised XMPP component

**Work:** implement XEP-0114 attachment, authentication and connection state;
validate ingress; run checked routing rules; encode and send the resulting
actions. Add supervision, bounded queues, reconnection and structured operational
errors. The first protocol proof may use loopback TCP. Remote deployment requires
a documented, validated protected transport configuration; the component
handshake alone does not provide encryption.

**Validation criteria:**

- The same packaged service authenticates, discovers and exchanges the selected
  stanzas with pinned ejabberd and Prosody configurations in separate runs.
- A valid alert reaches its configured recipients with the expected sender and
  content. Unauthorized senders, invalid payloads and unsupported IQ requests
  produce the documented outcome without a routing or error-response loop.
- Wrong credentials, connection rejection, remote closure, server restart,
  fragmented traffic, slow receivers and queue overflow behave as specified.
  Recovery completes within the P0 budget and does not grow queues indefinitely.
- A failed worker is observed and restarted according to the supervision policy;
  unrelated work continues where the design promises isolation. Logs expose the
  failure and recovery without recording credentials.
- Local send completion is not reported as recipient delivery. Disconnect and
  retry tests account for drops, duplicates and uncertain delivery according to
  the documented in-memory policy.
- The application core builds in P1's strict mode. Deliberately introducing a
  wrong field type, invalid return or missing event variant blocks that build;
  restoring the code produces a runnable component.

**Evidence:** server configurations, protocol traces with secrets removed,
fault-injection outcomes, bounded-queue measurements and strict build results.

### P5 Qualify deployment and measure the complete service

**Work:** package the executable, configuration instructions and required assets.
Qualify Linux x86-64 first, then Linux ARM64 on actual hardware. Document the
runtime/system library requirements; native output does not imply a fully static
link or zero operating-system dependencies.

**Validation criteria:**

- A clean build from pinned sources produces an identifiable artifact with
  checksums. It installs and runs outside the checkout on a clean target without
  Erlang/OTP, Nera, Rust or development headers installed.
- Configuration errors are actionable. Startup, graceful shutdown and restart
  are reproducible using the documented service-management procedure.
- Each claimed deployment target completes a proposed minimum 24-hour soak with
  steady traffic and at least 100 controlled reconnect cycles. There are no
  unexplained process failures or resource leaks, and resource/recovery budgets
  fixed in P0 hold. A cross-compiled object alone does not qualify a target.
- Report executable and total installed size, peak and steady RSS, idle CPU,
  connection readiness time, throughput, latency and queue depth, including the
  selected transport's encryption costs when applicable.
- Compare the same application behavior and workload on BEAM and Nera, with
  hardware, runtime versions, configuration, repetitions and raw samples recorded.
  Performance or footprint advantages are conclusions from those measurements,
  not acceptance assumptions.

**Evidence:** clean-host transcript, artifact hashes, target-specific soak and
benchmark reports, plus reproduction commands and any remaining limitations.

## Evidence and completion rules

Store the eventual runnable integration and instructions under a dedicated
`tests/integration/xmpp_component/` directory. This is a proposed location; this
roadmap does not create or claim an existing test harness.

Each validation record must contain:

- the Nera revision and source hashes for any uncommitted changes;
- upstream revisions, lockfile/dependency identities and adapter changes;
- platform, toolchain, server versions and sanitized configuration;
- exact commands, fixture counts, seeds, limits and time budgets;
- expected and observed results, including failures, skips and blocked cases;
- checked Erlang scope and unverified/native boundary assumptions;
- logs, measurements and artifact checksums sufficient to reproduce the result.

Mark a milestone passed only for its recorded scope and target. Update
[COMPATIBILITY.md](../COMPATIBILITY.md) and the relevant integration README when
execution evidence changes. Report a partial result as partial; one successful
stanza exchange cannot establish the entire milestone.

Implement the checks so they can run locally and later in CI. Follow the
[current contribution and CI policy](../CONTRIBUTING.md#temporary-private-ci-pause);
this roadmap does not authorize enabling paused workflows. Compiler/runtime
changes must also pass the repository's applicable regression and release gates.

The final demonstration must show, in order: identifiable Erlang source reuse;
a real type error rejected by the compiler; the corrected native component
routing an alert; and recovery from an injected connection or worker failure.
Publish the checking boundary and measured deployment properties alongside it.

## Later extensions

After this component is validated, the same foundations can support a corporate
S2S gateway, MQTT/XMPP client agents and Linux appliances. Durable delivery,
additional XMPP extensions, administration UI, typed inter-process protocols,
full-server extraction and microcontroller support each need separate scope and
acceptance criteria.

The earlier [mini-ejabberd design](MINI_EJABBERD.md) and
[flagship roadmap](FLAGSHIP_ROADMAP.md) remain broader explorations. For the first
XMPP demonstration, this external component takes precedence over building a
full client-facing server. The limited blocking checker in P1 also takes
precedence, for this demonstrator, over the historical type-enforcement deferral
in the [EEP roadmap](EEP_INFORMED_ROADMAP.md). General gradual typing remains a
separate, broader project.

## References

- [XEP-0114 component protocol](https://xmpp.org/extensions/xep-0114.html)
- [processone/xmpp library](https://github.com/processone/xmpp)
- [processone/fast_xml library](https://github.com/processone/fast_xml)
- [quick-xml reader API](https://docs.rs/quick-xml/latest/quick_xml/reader/struct.Reader.html)
- [Erlang and native library architecture](STDLIB_DESIGN.md)
- [ejabberd integration explorations](EJABBERD_USE_CASE.md)
