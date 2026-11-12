# Nera IR contract

This document describes the current `nera-ir` representation, not a proposed
SSA redesign. Read it alongside `crates/nera-ir/src/ir.rs`. The contract applies
to completed functions, after construction or a transformation, not to a
partially populated `IrBuilder`.

## Representation and identity

An `IrModule` contains functions and exports. An `IrFunction` contains an
explicit entry block, ordered function parameters, and a vector of basic blocks.
`BlockId` and `VarId` are function-local identities, not vector indices. They
need not be dense, and the entry block need not be first in the vector. Lowering
usually constructs the entry block last. Atom IDs belong to the compilation's
atom table; they are not runtime atom indices.

The IR uses **mutable virtual registers**, not strict static single assignment
(SSA). A fresh `VarId` names a register, but multiple instructions may write it.
In particular, alternative branches write a shared result with `Move`, and
loops can update existing registers. Cranelift's frontend variable API constructs
SSA when the backend translates these reads and writes. A structural verifier
must not reject repeated definitions simply because they are not SSA.

Function `arity` equals the length of `params`; parameter IDs are distinct.
Parameters are available on function entry. Lifted closures prepend captured
values to their ordinary arguments, so the lifted function's arity includes the
environment. `MakeClosure` separately records the user-visible arity.

## Blocks and control flow

Each block ID is unique within its function, and `entry_block` names a block in
that function. Every block contains an ordered instruction sequence and exactly
one terminator (required by the Rust data type). All referenced successor IDs
must exist, including references from unreachable blocks. Unreachable blocks
are allowed: construction and intermediate passes can leave them behind.

The current lowering emits empty `BasicBlock::params` and empty `Jump`
argument lists. These fields are reserved; the native translator does not bind
block parameters or pass jump arguments. They must remain empty in the current
executable IR contract. Function parameters live in `IrFunction::params`, not
in the entry block's reserved parameter list.

- `Return` reads its result and exits the function.
- `Jump` transfers to one block without arguments.
- `Branch` reads a condition; the backend compares it with the `true` atom.
- `Switch` describes literal cases and a default successor. Structural checks
  can inspect its edges and operand, but do not establish native backend
  support for every case literal. Current source lowering uses branches.
- `TailCall` reads its arguments and any dynamic target operands, then transfers
  out of this function's explicit CFG. A backend may implement a self-tail-call
  as a loop with parameter reassignment.
- `Unreachable` has no successors; the backend emits a trap.
- The high-level `Receive` variant is reserved and is not executable receive
  semantics. Source `receive` lowers to ordinary blocks and runtime calls such
  as `receive_peek`. The native translator's `Receive` arm is a placeholder.

Vector order is not execution order. Successors determine reachability. The
explicit CFG does not describe all backend-created exits, exception propagation,
or suspend/resume paths.

## Reads, writes and payloads

An instruction reads its operands before writing its results. A use in reachable
code must have a value on every explicit incoming path, either from a function
parameter or an earlier write. Definite assignment is a forward, intersection
analysis over the CFG, not a global "has some definition" or SSA dominance test.
A loop back-edge cannot initialize a register on the first iteration.

Most result-producing instructions write one register. `BsStartMatch` writes
two, `BsMatchStart2` writes three, and `BsGetUtf8/16/32` write two. Results of a
single multi-result instruction must have distinct IDs. `CallTarget::Closure`
and `CallTarget::Apply` contain read operands in addition to the argument list.
Closure environments are reads as well. Binary writes, binary resize and
match-context advancement mutate runtime data without defining a register;
UTF writes additionally produce a bit-count result.

Values are generally tagged Erlang terms, but the IR is not statically typed:
binary matching instructions also carry untagged data pointers. Structural
verification cannot prove operand types, pointer validity, rooting or lifetime.
`MakeMap` uses interleaved key/value operands and requires an even count.
`BinFlags` encodes signedness as 0 or 1 and endianness as 0, 1 or 2.

## Exceptions, scheduling and source information

`RaiseException` writes a result register and changes runtime exception state;
it is an instruction, not a terminator. `CheckException`, exception accessors,
`ClearException` and ordinary branches express part of exception handling.
Calls can also trigger backend-generated exception checks and suspension.
`CheckReductions` and `YieldCheck` are distinct scheduling operations, not pure
computations. Do not remove or reorder them merely because they have no result.

`IrFunction::line` and `SetLine` supply stack-trace locations; zero means unknown
for the function line. The IR does not currently retain a source span on every
instruction. Verifier locations should therefore identify the function, block
and instruction index rather than inventing a precise source span.

## Verification boundary

The structural verifier is a read-only development API. It checks completed
functions without rewriting or repairing them. It is separate from
`validate::check_unresolved_calls`, which resolves calls across modules and BIFs.
It is not inserted into the production pipeline in this milestone, so compiler
acceptance, diagnostics, optimization order and generated code remain unchanged.
Tests exercise the verifier on malformed fixtures and real lowering output.

Checks cover function parameter shape, block identity and edges, reserved
fields/terminators, instruction payload shape, register declarations throughout
the function, and definite assignment on entry-reachable explicit CFG paths.
Unreachable code is checked for undeclared registers but not path-sensitive
initialization: there is no entry state for an unreachable component.

Success does **not** prove Erlang semantics, static types, call resolution or
call arities, closure target linkage, atom-table validity, exception-handler
balance, suspension safety, GC safety, optimization correctness, or backend
support for every representable operation. Those require separate checks.

When adding an instruction, update its documented operands/results, exhaustive
verifier operand handling and positive/negative tests. Verify before and after a
transformation in its tests; do not silently strengthen the accepted IR contract
without checking existing lowering and backend consumers.

The existing transformations and their ordering are documented in
[IR passes](IR_PASSES.md). Unit-test builds verify each pipeline boundary.
