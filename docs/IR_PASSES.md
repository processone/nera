# IR passes

`nera_ir::passes` owns the existing IR transformations. AST construction remains
in `lower`; `lower_function` hands each completed source function to
`passes::run_function_passes`. The individual functions remain available through
`nera_ir::lower` as compatibility re-exports.

## Order and scope

The source-function pipeline runs once, in this order:

| Step | Pass | Existing effect |
| --- | --- | --- |
| 1 | `insert_backedge_yield_checks` | Append scheduling checks at DFS back-edge sources when needed. |
| 2 | `fuse_make_tuple_send` | Replace eligible tuple construction and send pairs with `SendDirect`, retaining a nil definition of the tuple register. |
| 3 | `eliminate_dead_match_contexts` | Replace `BsMatchStart2` with `BsStartMatch` when the context result has no uses. |
| 4 | `fold_constants` | Fold supported operations from constants tracked within each block. |
| 5 | `eliminate_dead_code` | Simplify constant branches/switches, then retain entry-reachable blocks. |

Lifted lambdas and list-comprehension helper functions receive **only back-edge
yield insertion**, at their existing construction sites. They do not enter the
source-function pipeline. This distinction is preserved from the original
lowering code, rather than being an optimization policy added by extraction.

Order is observable in the IR. Constant folding exposes branches for dead-code
elimination. A scheduling check inserted at an original back-edge remains even
if later folding eliminates that edge. The pipeline regression test checks both
interactions alongside tuple/send fusion and match-context removal.

## Boundaries

Each pass mutates one completed `IrFunction` in place. Folding and dead-code
elimination additionally receive the module's true/false atom IDs. Passes do not
allocate registers or blocks, change function signatures, or resolve calls.
Only dead-code elimination changes the CFG; the other passes change instructions.
Each invocation builds its own local maps and sets. There are no cached analyses
or invalidation protocol.

`uses.rs` contains the operand collectors shared by tuple/send fusion and
match-context removal. Their existing traversal and counting behavior is
preserved. The structural verifier keeps its own operand handling; extracting
passes does not unify these analyses or alter optimization eligibility.

## Verification

The [IR contract](IR_CONTRACT.md) applies before and after every pass. In unit-test
builds, the source-function pipeline verifies the input and every intermediate
result. Generated helper sites likewise verify before and after yield insertion.
Direct pass regression tests verify their fixtures before and after mutation.
Production builds do not run these checks, preserving compiler acceptance and
diagnostics.

The extraction was validated one pass at a time against the original pipeline:
all 20 real lowering fixtures produced identical full IR after each step. These
comparisons supplement the structural checks: verifier success alone does not
prove unchanged semantics. Future pass changes need focused behavioral tests
and end-to-end compiler tests as well.
