//! Existing block-local constant evaluation.

use crate::ir::{Instruction, IrFunction, Literal, VarId};
use nera_syntax::ast::{AtomId, BinOp, UnaryOp};

/// Constant folding pass: evaluate compile-time-known arithmetic, bitwise,
/// comparison, and unary operations, replacing sequences like
/// `Const(v0, 1), Const(v1, 2), BinOp(v2, Add, v0, v1)` with `Const(v2, 3)`.
pub fn fold_constants(func: &mut IrFunction, true_atom_id: AtomId, false_atom_id: AtomId) {
    use std::collections::HashMap;

    for block in &mut func.blocks {
        let mut known: HashMap<VarId, Literal> = HashMap::new();

        for inst in &mut block.instructions {
            match inst {
                Instruction::Const(var, lit) => {
                    known.insert(*var, lit.clone());
                }
                Instruction::BinOp(result, op, lhs, rhs) => {
                    if let (Some(lv), Some(rv)) = (known.get(lhs), known.get(rhs)) {
                        if let Some(folded) = fold_binop(*op, lv, rv, true_atom_id, false_atom_id) {
                            let r = *result;
                            known.insert(r, folded.clone());
                            *inst = Instruction::Const(r, folded);
                        }
                    }
                }
                Instruction::UnaryOp(result, op, operand) => {
                    if let Some(val) = known.get(operand) {
                        if let Some(folded) = fold_unaryop(*op, val, true_atom_id, false_atom_id) {
                            let r = *result;
                            known.insert(r, folded.clone());
                            *inst = Instruction::Const(r, folded);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

/// Try to evaluate a binary operation on two known literals at compile time.
fn fold_binop(
    op: BinOp,
    lhs: &Literal,
    rhs: &Literal,
    true_atom_id: AtomId,
    false_atom_id: AtomId,
) -> Option<Literal> {
    use nera_syntax::ast::BinOp::*;

    match (op, lhs, rhs) {
        // Integer arithmetic with overflow check
        (Add, Literal::Integer(a), Literal::Integer(b)) => {
            i64::checked_add(*a, *b).map(Literal::Integer)
        }
        (Sub, Literal::Integer(a), Literal::Integer(b)) => {
            i64::checked_sub(*a, *b).map(Literal::Integer)
        }
        (Mul, Literal::Integer(a), Literal::Integer(b)) => {
            i64::checked_mul(*a, *b).map(Literal::Integer)
        }
        // Integer division / remainder — skip if divisor is 0
        (IntDiv, Literal::Integer(a), Literal::Integer(b)) => {
            if *b == 0 {
                None
            } else {
                i64::checked_div(*a, *b).map(Literal::Integer)
            }
        }
        (Rem, Literal::Integer(a), Literal::Integer(b)) => {
            if *b == 0 {
                None
            } else {
                i64::checked_rem(*a, *b).map(Literal::Integer)
            }
        }
        // Float division and arithmetic with at least one float. An infinite
        // or NaN result, which includes every zero divisor, is not folded:
        // the operation then runs and raises `badarith`, as on BEAM.
        (Div, Literal::Integer(a), Literal::Integer(b)) => finite_float(*a as f64 / *b as f64),
        (Div, Literal::Float(a), Literal::Integer(b)) => finite_float(*a / *b as f64),
        (Div, Literal::Integer(a), Literal::Float(b)) => finite_float(*a as f64 / *b),
        (Div, Literal::Float(a), Literal::Float(b)) => finite_float(*a / *b),
        (Add, Literal::Float(a), Literal::Float(b)) => finite_float(*a + *b),
        (Add, Literal::Float(a), Literal::Integer(b)) => finite_float(*a + *b as f64),
        (Add, Literal::Integer(a), Literal::Float(b)) => finite_float(*a as f64 + *b),
        (Sub, Literal::Float(a), Literal::Float(b)) => finite_float(*a - *b),
        (Sub, Literal::Float(a), Literal::Integer(b)) => finite_float(*a - *b as f64),
        (Sub, Literal::Integer(a), Literal::Float(b)) => finite_float(*a as f64 - *b),
        (Mul, Literal::Float(a), Literal::Float(b)) => finite_float(*a * *b),
        (Mul, Literal::Float(a), Literal::Integer(b)) => finite_float(*a * *b as f64),
        (Mul, Literal::Integer(a), Literal::Float(b)) => finite_float(*a as f64 * *b),

        // Integer bitwise ops
        (Band, Literal::Integer(a), Literal::Integer(b)) => Some(Literal::Integer(*a & *b)),
        (Bor, Literal::Integer(a), Literal::Integer(b)) => Some(Literal::Integer(*a | *b)),
        (Bxor, Literal::Integer(a), Literal::Integer(b)) => Some(Literal::Integer(*a ^ *b)),
        (Bsl, Literal::Integer(a), Literal::Integer(b)) => {
            if *b < 0 || *b > 63 {
                None
            } else {
                // Shift in a wider type: checking only the shift count would
                // still discard significant bits in an i64 result.
                i64::try_from((*a as i128) << *b).ok().map(Literal::Integer)
            }
        }
        (Bsr, Literal::Integer(a), Literal::Integer(b)) => {
            if *b < 0 || *b > 63 {
                None
            } else {
                Some(Literal::Integer(*a >> *b))
            }
        }

        // Comparisons — integer vs integer
        (Eq, Literal::Integer(a), Literal::Integer(b))
        | (ExactEq, Literal::Integer(a), Literal::Integer(b)) => {
            Some(bool_literal(a == b, true_atom_id, false_atom_id))
        }
        (NotEq, Literal::Integer(a), Literal::Integer(b))
        | (ExactNotEq, Literal::Integer(a), Literal::Integer(b)) => {
            Some(bool_literal(a != b, true_atom_id, false_atom_id))
        }
        (Lt, Literal::Integer(a), Literal::Integer(b)) => {
            Some(bool_literal(a < b, true_atom_id, false_atom_id))
        }
        (Gt, Literal::Integer(a), Literal::Integer(b)) => {
            Some(bool_literal(a > b, true_atom_id, false_atom_id))
        }
        (LtEq, Literal::Integer(a), Literal::Integer(b)) => {
            Some(bool_literal(a <= b, true_atom_id, false_atom_id))
        }
        (GtEq, Literal::Integer(a), Literal::Integer(b)) => {
            Some(bool_literal(a >= b, true_atom_id, false_atom_id))
        }

        // Comparisons — float vs float
        (Eq, Literal::Float(a), Literal::Float(b)) => {
            Some(bool_literal(a == b, true_atom_id, false_atom_id))
        }
        (ExactEq, Literal::Float(a), Literal::Float(b)) => {
            Some(bool_literal(a.to_bits() == b.to_bits(), true_atom_id, false_atom_id))
        }
        (NotEq, Literal::Float(a), Literal::Float(b)) => {
            Some(bool_literal(a != b, true_atom_id, false_atom_id))
        }
        (ExactNotEq, Literal::Float(a), Literal::Float(b)) => {
            Some(bool_literal(a.to_bits() != b.to_bits(), true_atom_id, false_atom_id))
        }
        (Lt, Literal::Float(a), Literal::Float(b)) => {
            Some(bool_literal(a < b, true_atom_id, false_atom_id))
        }
        (Gt, Literal::Float(a), Literal::Float(b)) => {
            Some(bool_literal(a > b, true_atom_id, false_atom_id))
        }
        (LtEq, Literal::Float(a), Literal::Float(b)) => {
            Some(bool_literal(a <= b, true_atom_id, false_atom_id))
        }
        (GtEq, Literal::Float(a), Literal::Float(b)) => {
            Some(bool_literal(a >= b, true_atom_id, false_atom_id))
        }

        _ => None,
    }
}

/// Try to evaluate a unary operation on a known literal at compile time.
fn fold_unaryop(
    op: UnaryOp,
    val: &Literal,
    true_atom_id: AtomId,
    false_atom_id: AtomId,
) -> Option<Literal> {
    use nera_syntax::ast::UnaryOp::*;

    match (op, val) {
        (Neg, Literal::Integer(n)) => i64::checked_neg(*n).map(Literal::Integer),
        (Neg, Literal::Float(f)) => Some(Literal::Float(-f)),
        (Not, Literal::Atom(id)) if id == &true_atom_id => Some(Literal::Atom(false_atom_id)),
        (Not, Literal::Atom(id)) if id == &false_atom_id => Some(Literal::Atom(true_atom_id)),
        (Bnot, Literal::Integer(n)) => Some(Literal::Integer(!n)),
        _ => None,
    }
}

/// A folded float result, or no fold when it is infinite or NaN.
fn finite_float(value: f64) -> Option<Literal> {
    value.is_finite().then_some(Literal::Float(value))
}

/// Produce a boolean atom literal.
fn bool_literal(val: bool, true_atom_id: AtomId, false_atom_id: AtomId) -> Literal {
    Literal::Atom(if val { true_atom_id } else { false_atom_id })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn left_shift_folding_preserves_signed_boundaries() {
        for (value, shift, expected) in [
            (1, 62, Some(4611686018427387904)),
            (1, 63, None),
            (-1, 63, Some(i64::MIN)),
            (16, 60, None),
            (-16, 60, None),
            (i64::MAX, 0, Some(i64::MAX)),
            (i64::MAX, 1, None),
            (i64::MIN, 0, Some(i64::MIN)),
            (i64::MIN, 1, None),
            (0, 63, Some(0)),
            (7, 3, Some(56)),
            (-7, 3, Some(-56)),
            (1, -1, None),
            (1, 64, None),
        ] {
            let folded = fold_binop(
                BinOp::Bsl, &Literal::Integer(value), &Literal::Integer(shift),
                AtomId(0), AtomId(1),
            );
            let actual = folded.map(|literal| match literal {
                Literal::Integer(value) => value,
                _ => panic!("expected integer literal"),
            });
            assert_eq!(actual, expected, "{value} bsl {shift}");
        }
    }
}
