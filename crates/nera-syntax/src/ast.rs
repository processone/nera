use std::collections::HashSet;

use crate::span::Span;

/// Interned atom identifier. Wraps a `string_interner::DefaultSymbol` index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AtomId(pub u32);

/// A complete Erlang module.
#[derive(Debug, Clone)]
pub struct Module {
    pub name: AtomId,
    /// Span covering just the module name atom in `-module(Name).`
    pub name_span: Span,
    pub exports: Vec<FunArity>,
    pub imports: Vec<ImportDecl>,
    pub no_auto_import: NoAutoImport,
    pub type_exports: Vec<FunArity>,
    pub deprecated: Vec<FunArity>,
    pub functions: Vec<Function>,
    pub records: Vec<RecordDef>,
    pub type_defs: Vec<TypeDef>,
    pub specs: Vec<SpecDef>,
    pub behaviours: Vec<BehaviourAttr>,
    /// Type names referenced in `-spec`, `-callback`, and `-type`/`-opaque` bodies.
    /// Collected during parsing as atoms followed by `(` in type contexts.
    pub type_refs: HashSet<AtomId>,
    pub span: Span,
}

/// Module-level auto-import suppression from compile options.
#[derive(Debug, Clone, Default)]
pub struct NoAutoImport {
    pub all: bool,
    pub functions: Vec<FunArity>,
}

/// Functions made available without a module qualifier by `-import`.
#[derive(Debug, Clone)]
pub struct ImportDecl {
    pub module: AtomId,
    pub functions: Vec<FunArity>,
}

/// A `-behaviour(name).` or `-behavior(name).` attribute.
#[derive(Debug, Clone)]
pub struct BehaviourAttr {
    pub name: AtomId,
    pub span: Span,
}

/// A `-type` or `-opaque` attribute definition.
#[derive(Debug, Clone)]
pub struct TypeDef {
    pub name: AtomId,
    pub name_span: Span,
    pub arity: u32,
    pub is_opaque: bool,
    pub span: Span,
}

/// A `-spec` or `-callback` attribute definition.
#[derive(Debug, Clone)]
pub struct SpecDef {
    pub func_name: AtomId,
    pub name_span: Span,
    pub arity: u32,
    pub is_callback: bool,
    pub span: Span,
}

/// A function name / arity pair, e.g. `foo/2`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunArity {
    pub name: AtomId,
    pub arity: u32,
    pub span: Span,
}

/// A top-level function definition (all clauses sharing the same name/arity).
#[derive(Debug, Clone)]
pub struct Function {
    pub name: AtomId,
    /// Span covering just the function name atom (for semantic highlighting).
    pub name_span: Span,
    pub arity: u32,
    pub clauses: Vec<FunctionClause>,
    pub span: Span,
}

/// One clause of a function definition.
#[derive(Debug, Clone)]
pub struct FunctionClause {
    /// Span covering the function name atom for this clause.
    pub name_span: Span,
    pub patterns: Vec<Pattern>,
    pub guard: Option<Guard>,
    pub body: Vec<Expr>,
    pub span: Span,
}

/// Guard expressions.
#[derive(Debug, Clone)]
pub enum Guard {
    /// A single guard test expression.
    Test(Box<Expr>),
    /// Comma-separated conjunction of guard tests.
    Conjunction(Vec<Expr>),
    /// Semicolon-separated disjunction of guard conjunctions.
    Disjunction(Vec<Guard>),
}

/// Pattern nodes used in function heads, case clauses, and match expressions.
#[derive(Debug, Clone)]
pub enum Pattern {
    /// The wildcard pattern `_`.
    Wildcard(Span),
    /// A variable pattern, e.g. `X`.
    Variable(AtomId, Span),
    /// An integer literal pattern.
    Integer(i64, Span),
    /// Canonical decimal integer outside i64.
    BigInteger(String, Span),
    /// A float literal pattern.
    Float(f64, Span),
    /// An atom literal pattern.
    Atom(AtomId, Span),
    /// A string literal pattern.
    String(String, Span),
    /// A tuple pattern, e.g. `{ok, X}`.
    Tuple(Vec<Pattern>, Span),
    /// A cons cell pattern, e.g. `[H|T]`.
    Cons(Box<Pattern>, Box<Pattern>, Span),
    /// The empty list `[]`.
    Nil(Span),
    /// An alias / match pattern, e.g. `X = {ok, V}`.
    Match(Box<Pattern>, Box<Pattern>, Span),
    /// A binary pattern, e.g. `<<X:8, Y:16, Rest/binary>>`.
    Binary(Vec<BinSegment<Pattern>>, Span),
    /// A map pattern, e.g. `#{key := Value}`.
    Map(Vec<MapPatternField>, Span),
    /// A record pattern, e.g. `#person{name = N, age = _}`.
    Record {
        record_name: AtomId,
        fields: Vec<RecordPatternField>,
        span: Span,
    },
    /// String prefix match pattern: `"prefix" ++ Rest`.
    /// LHS is a string literal, RHS is an arbitrary pattern (typically a variable).
    /// Desugared to nested `Cons` cells in the pattern compiler.
    Concat(Box<Pattern>, Box<Pattern>, Span),
    /// An unexpanded macro call in pattern position: `?NAME` or `?NAME(...)`.
    /// Only produced when parsing without preprocessor (e.g. formatter path).
    MacroCall {
        name: AtomId,
        args: Option<Vec<Pattern>>,
        span: Span,
    },
}

/// A single field in a map pattern: `KeyExpr := ValuePattern`.
#[derive(Debug, Clone)]
pub struct MapPatternField {
    pub key: Expr,
    pub value: Pattern,
    pub span: Span,
}

/// An item in a `maybe` block: either a plain expression or a conditional match (`?=`).
#[derive(Debug, Clone)]
pub enum MaybeExpr {
    /// A plain expression in the maybe body.
    Expr(Expr),
    /// A conditional match: `Pattern ?= Expr`.
    CondMatch(Pattern, Expr, Span),
}

/// Expression nodes.
#[derive(Debug, Clone)]
pub enum Expr {
    Integer(i64, Span),
    /// Canonical decimal integer outside i64.
    BigInteger(String, Span),
    Float(f64, Span),
    Atom(AtomId, Span),
    String(String, Span),
    Nil(Span),
    Variable(AtomId, Span),
    Tuple(Vec<Expr>, Span),
    Cons(Box<Expr>, Box<Expr>, Span),
    BinOp(BinOp, Box<Expr>, Box<Expr>, Span),
    UnaryOp(UnaryOp, Box<Expr>, Span),
    Match(Box<Pattern>, Box<Expr>, Span),
    Case(Box<Expr>, Vec<CaseClause>, Span),
    If(Vec<IfClause>, Span),
    Receive(Vec<CaseClause>, Option<AfterClause>, Span),
    Call(CallTarget, Vec<Expr>, Span),
    Send(Box<Expr>, Box<Expr>, Span),
    Block(Vec<Expr>, Span),
    /// A binary expression, e.g. `<<1, 2, X:16>>`.
    Binary(Vec<BinSegment<Expr>>, Span),
    /// A map expression, e.g. `#{key => value}`.
    Map(Vec<MapField>, Span),
    /// A map update expression, e.g. `M#{key => value, key2 := value2}`.
    MapUpdate(Box<Expr>, Vec<MapUpdateField>, Span),
    /// A list comprehension, e.g. `[Expr || Var <- List, Guard]`.
    ListComp(Box<Expr>, Vec<LcQualifier>, Span),
    /// A binary comprehension, e.g. `<< <<X>> || X <- List >>`.
    BinComp(Box<Expr>, Vec<LcQualifier>, Span),
    /// A try/catch/after expression, optionally with `of` clauses.
    Try {
        body: Vec<Expr>,
        of_clauses: Vec<CaseClause>,
        catch_clauses: Vec<CatchClause>,
        after_body: Option<Vec<Expr>>,
        span: Span,
    },
    /// `throw(Reason)` — raises a throw exception.
    Throw(Box<Expr>, Span),
    /// `catch Expr` — unary catch operator (returns value or `{'EXIT', ...}`).
    Catch(Box<Expr>, Span),
    /// Anonymous or named fun: `fun(X) -> X + 1 end` or `fun Name(X) -> X end`.
    /// The optional `AtomId` is the name for named funs (used for self-recursion).
    Fun(Option<AtomId>, Vec<FunctionClause>, Span),
    /// Function reference: `fun name/arity`.
    FunRef(FunRefTarget, Span),
    /// Record creation: `#person{name = "Alice", age = 30}`.
    RecordCreate {
        record_name: AtomId,
        fields: Vec<RecordFieldAssign>,
        span: Span,
    },
    /// Record update: `R#person{name = "Bob"}`.
    RecordUpdate {
        base: Box<Expr>,
        record_name: AtomId,
        fields: Vec<RecordFieldAssign>,
        span: Span,
    },
    /// Record field access: `R#person.name`.
    RecordAccess {
        base: Box<Expr>,
        record_name: AtomId,
        field_name: AtomId,
        field_name_span: Span,
        span: Span,
    },
    /// Record index: `#person.name` → integer (1-based Erlang field index).
    RecordIndex {
        record_name: AtomId,
        field_name: AtomId,
        field_name_span: Span,
        span: Span,
    },
    /// A `maybe...end` expression (EEP-49, OTP 25+).
    Maybe {
        body: Vec<MaybeExpr>,
        else_clauses: Vec<CaseClause>,
        span: Span,
    },
    /// An unexpanded macro call: `?NAME` or `?NAME(arg1, arg2, ...)`.
    /// Only produced when parsing without preprocessor (e.g. formatter path).
    MacroCall {
        name: AtomId,
        args: Option<Vec<Expr>>,
        span: Span,
    },
}

impl Expr {
    /// Return the source span of this expression.
    pub fn span(&self) -> Span {
        match self {
            Expr::Integer(_, s)
            | Expr::BigInteger(_, s)
            | Expr::Float(_, s)
            | Expr::Atom(_, s)
            | Expr::String(_, s)
            | Expr::Nil(s)
            | Expr::Variable(_, s)
            | Expr::Tuple(_, s)
            | Expr::Cons(_, _, s)
            | Expr::BinOp(_, _, _, s)
            | Expr::UnaryOp(_, _, s)
            | Expr::Match(_, _, s)
            | Expr::Case(_, _, s)
            | Expr::If(_, s)
            | Expr::Receive(_, _, s)
            | Expr::Call(_, _, s)
            | Expr::Send(_, _, s)
            | Expr::Block(_, s)
            | Expr::Binary(_, s)
            | Expr::Map(_, s)
            | Expr::MapUpdate(_, _, s)
            | Expr::ListComp(_, _, s)
            | Expr::BinComp(_, _, s)
            | Expr::Throw(_, s)
            | Expr::Catch(_, s)
            | Expr::Fun(_, _, s)
            | Expr::FunRef(_, s) => *s,
            Expr::Try { span, .. }
            | Expr::RecordCreate { span, .. }
            | Expr::RecordUpdate { span, .. }
            | Expr::RecordAccess { span, .. }
            | Expr::RecordIndex { span, .. }
            | Expr::Maybe { span, .. }
            | Expr::MacroCall { span, .. } => *span,
        }
    }
}

/// A qualifier in a list or binary comprehension.
#[derive(Debug, Clone)]
pub enum LcQualifier {
    /// Generator: `Pattern <- Expr`.
    Generator(Pattern, Box<Expr>, Span),
    /// Binary generator: `<<Pattern>> <= Expr`.
    BinGenerator(Pattern, Box<Expr>, Span),
    /// Filter: a boolean expression.
    Filter(Box<Expr>, Span),
}

/// Operator used in a map field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapFieldOp {
    /// `=>` — association (map construction)
    Assoc,
    /// `:=` — exact match (map pattern / update)
    Exact,
}

/// A single field in a map expression: `KeyExpr => ValueExpr` or `KeyExpr := ValueExpr`.
#[derive(Debug, Clone)]
pub struct MapField {
    pub key: Expr,
    pub value: Expr,
    pub op: MapFieldOp,
    pub op_span: Span,
    pub span: Span,
}

/// Operator in a map update field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapUpdateOp {
    /// `=>` — insert or replace (key may or may not exist)
    Assoc,
    /// `:=` — exact update (key must exist, otherwise `badkey`)
    Exact,
}

/// A single field in a map update expression: `KeyExpr Op ValueExpr`.
#[derive(Debug, Clone)]
pub struct MapUpdateField {
    pub key: Expr,
    pub value: Expr,
    pub op: MapUpdateOp,
    pub span: Span,
}

// ── Record definitions and fields ──────────────────────────────────────

/// A field definition in a `-record(name, {fields...}).` attribute.
#[derive(Debug, Clone)]
pub struct RecordFieldDef {
    /// Field name (an atom).
    pub name: AtomId,
    /// Span of the field name atom.
    pub name_span: Span,
    /// Default value expression, or None (defaults to `undefined`).
    pub default: Option<Expr>,
    pub span: Span,
}

/// A complete record definition from `-record(name, {fields...}).`
#[derive(Debug, Clone)]
pub struct RecordDef {
    /// Record name (an atom).
    pub name: AtomId,
    /// Span of the record name atom.
    pub name_span: Span,
    /// Ordered list of field definitions.
    pub fields: Vec<RecordFieldDef>,
    pub span: Span,
}

/// A field assignment in a record creation or update expression.
#[derive(Debug, Clone)]
pub struct RecordFieldAssign {
    pub field_name: AtomId,
    /// Span of the field name atom.
    pub field_name_span: Span,
    pub value: Expr,
    pub span: Span,
}

/// A field in a record pattern.
#[derive(Debug, Clone)]
pub struct RecordPatternField {
    pub field_name: AtomId,
    /// Span of the field name atom.
    pub field_name_span: Span,
    pub pattern: Pattern,
    pub span: Span,
}

/// A clause in a `catch` block: `Class:Pattern[:Stacktrace] [when Guard] -> Body`.
#[derive(Debug, Clone)]
pub struct CatchClause {
    pub class: CatchClass,
    pub pattern: Pattern,
    /// Optional stacktrace binding (3-part catch: `Class:Reason:Stacktrace`).
    pub stacktrace: Option<Pattern>,
    pub guard: Option<Guard>,
    pub body: Vec<Expr>,
    pub span: Span,
}

/// Exception class in a catch clause.
#[derive(Debug, Clone)]
pub enum CatchClass {
    Throw,
    Error,
    Exit,
    /// Variable binding for the class, e.g. `Class:Pattern`.
    Variable(AtomId, Span),
}

/// The target of a function call.
#[derive(Debug, Clone)]
pub enum CallTarget {
    /// Local call, e.g. `foo(1)`. The span covers the function name atom.
    Local(AtomId, Span),
    /// Remote call, e.g. `mod:fun(1)` or `Mod:Fun(1)`.
    Remote(Box<Expr>, Box<Expr>),
    /// Variable call, e.g. `F(1)` where F holds a fun. The span covers the variable.
    Variable(AtomId, Span),
    /// Call whose callee is an arbitrary expression, e.g.
    /// `(fun() -> 42 end)(0)` or `(make_fun())(x)`.
    Expr(Box<Expr>),
}

/// The target of a function reference (`fun name/arity` or `fun M:F/A`).
#[derive(Debug, Clone)]
pub enum FunRefTarget {
    /// Local function reference: `fun name/arity`.
    Local(AtomId, u32),
    /// Remote function reference: `fun Module:Function/Arity`.
    Remote(AtomId, AtomId, u32),
    /// Dynamic remote function reference: `fun Mod:Fun/Arity` where components are expressions.
    DynamicRemote(Box<Expr>, Box<Expr>, Box<Expr>),
}

/// A clause in a `case` or `receive` expression.
#[derive(Debug, Clone)]
pub struct CaseClause {
    pub pattern: Pattern,
    pub guard: Option<Guard>,
    pub body: Vec<Expr>,
    pub span: Span,
}

/// A clause in an `if` expression.
#[derive(Debug, Clone)]
pub struct IfClause {
    pub guard: Guard,
    pub body: Vec<Expr>,
    pub span: Span,
}

/// The `after` clause of a `receive` expression.
#[derive(Debug, Clone)]
pub struct AfterClause {
    pub timeout: Box<Expr>,
    pub body: Vec<Expr>,
    pub span: Span,
}

/// A single segment in a binary expression or pattern.
/// Represents: `Value:Size/TypeSpecifiers`
#[derive(Debug, Clone)]
pub struct BinSegment<T> {
    /// The value: an expression (for construction) or pattern (for matching)
    pub value: T,
    /// Size in bits (None = default: 8 for integer, all remaining for binary)
    pub size: Option<Box<Expr>>,
    /// Type specifier
    pub type_spec: BinSegmentType,
    pub span: Span,
}

/// Binary segment type specifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinSegmentType {
    pub typ: BinSegmentKind,
    pub signedness: Signedness,
    pub endianness: Endianness,
    /// Only a written `unit:N`; alias units are represented by `typ`.
    pub unit: Option<u32>,
    /// Whether a `/Type-Specifier` list was written. OTP refuses a string
    /// literal in a binary pattern with any list that names no UTF type,
    /// even one of defaults such as `"ab"/integer`.
    pub explicit: bool,
    /// Written declarations, before defaults. Semantic validation owns their
    /// merge policy; the effective fields above are used only after it passes.
    pub specifiers: Vec<BinSpecifier>,
    /// The exact written list, excluding `/`, for lossless formatting.
    pub specifiers_span: Option<Span>,
}

impl Default for BinSegmentType {
    fn default() -> Self {
        BinSegmentType {
            typ: BinSegmentKind::Integer,
            signedness: Signedness::Unsigned,
            endianness: Endianness::Big,
            unit: None,
            explicit: false,
            specifiers: Vec::new(),
            specifiers_span: None,
        }
    }
}

/// One written binary segment declaration. Aliases remain distinct where
/// they declare a unit: `bytes` is binary/unit:8, `bits` is bitstring/unit:1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinSpecifier {
    Type(BinSegmentKind),
    Bytes,
    Signedness(Signedness),
    Endianness(Endianness),
    Unit(u32),
}

/// Binary segment type kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinSegmentKind {
    Integer,
    Float,
    Binary,
    /// `bitstring` / `bits` — like `binary` but with default unit:1 instead of unit:8.
    Bitstring,
    Utf8,
    Utf16,
    Utf32,
}

/// Signedness for binary segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signedness {
    Unsigned,
    Signed,
}

impl std::fmt::Display for Signedness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Unsigned => "unsigned",
            Self::Signed => "signed",
        })
    }
}

/// Endianness for binary segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endianness {
    Big,
    Little,
    Native,
}

impl std::fmt::Display for Endianness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Big => "big",
            Self::Little => "little",
            Self::Native => "native",
        })
    }
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    IntDiv,
    Rem,
    Eq,
    ExactEq,
    NotEq,
    ExactNotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    And,
    Or,
    Xor,
    Andalso,
    Orelse,
    Band,
    Bor,
    Bxor,
    Bsl,
    Bsr,
    ListAppend,
    ListSubtract,
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
    Bnot,
}
