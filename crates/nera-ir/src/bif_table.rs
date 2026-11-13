//! Unified BIF (Built-In Function) registry.
//!
//! Native capability and editor metadata, separate from the OTP name catalogue:
//! - **IR lowering** uses the separate [`crate::resolution`] policy and
//!   [`crate::otp_auto_imports`] catalogue to resolve unqualified calls.
//! - **Code generation** uses [`lookup_ffi_symbol`] to map qualified remote calls
//!   to their FFI symbol names (e.g. `erlang:length/1` → `nera_rt_length`).
//! - **LSP inlay hints** uses [`lookup_bif_param_names`] to show parameter names
//!   for BIF calls (e.g. `element(2, T)` → `N:`, `Tuple:`).
//!
//! Adding an implementation does not change language-level auto-import names.

/// A single BIF entry in the unified table.
#[derive(Debug, Clone, Copy)]
pub struct BifEntry {
    /// Module name (e.g. "erlang", "io", "maps", "gen_tcp").
    pub module: &'static str,
    /// Function name (e.g. "length", "hd", "format").
    pub function: &'static str,
    /// Number of arguments.
    pub arity: u32,
    /// FFI symbol for direct codegen dispatch (e.g. "nera_rt_length").
    /// `None` means the call falls through to `nera_rt_apply` at runtime.
    pub ffi_symbol: Option<&'static str>,
    /// Human-readable parameter names for LSP inlay hints.
    /// `None` for BIFs where hints add no value (single-arg type checks, etc.).
    pub param_names: Option<&'static [&'static str]>,
    /// Erlang-style type spec string (e.g. "-spec abs(Number) -> Number when Number :: number().").
    /// Used by LSP hover and future type checking.
    pub spec: Option<&'static str>,
}

/// Native capability and editor metadata for implemented BIFs.
///
/// Each entry combines native FFI mapping and editor metadata. The table contains
/// BIFs known to the compiler.
pub const BIF_TABLE: &[BifEntry] = &[
    BifEntry { module: "base64", function: "decode_list", arity: 3, ffi_symbol: Some("nera_rt_base64_decode_list"), param_names: Some(&["Data", "UrlSafe", "Padding"]), spec: Some("-spec decode_list(Data :: list(), UrlSafe :: boolean(), Padding :: boolean()) -> binary().") },
    BifEntry { module: "base64", function: "encode_binary", arity: 3, ffi_symbol: Some("nera_rt_base64_encode_binary"), param_names: Some(&["Data", "UrlSafe", "Padding"]), spec: Some("-spec encode_binary(Data :: binary(), UrlSafe :: boolean(), Padding :: boolean()) -> binary().") },
    BifEntry { module: "base64", function: "decode_binary", arity: 3, ffi_symbol: Some("nera_rt_base64_decode_binary"), param_names: Some(&["Data", "UrlSafe", "Padding"]), spec: Some("-spec decode_binary(Data :: binary(), UrlSafe :: boolean(), Padding :: boolean()) -> binary().") },
    // -------------------------------------------------------------------------
    // erlang module — arithmetic
    // -------------------------------------------------------------------------
    BifEntry { module: "erlang", function: "abs",   arity: 1, ffi_symbol: Some("nera_rt_abs"), param_names: None, spec: Some("-spec abs(Float) -> float(); (Int) -> non_neg_integer().") },
    BifEntry { module: "math", function: "sqrt", arity: 1, ffi_symbol: Some("nera_rt_math_sqrt"), param_names: Some(&["Number"]), spec: Some("-spec sqrt(Number :: number()) -> float().") },
    BifEntry { module: "math", function: "exp", arity: 1, ffi_symbol: Some("nera_rt_math_exp"), param_names: Some(&["X"]), spec: Some("-spec exp(X :: number()) -> float().") },
    BifEntry { module: "math", function: "log", arity: 1, ffi_symbol: Some("nera_rt_math_log"), param_names: Some(&["X"]), spec: Some("-spec log(X :: number()) -> float().") },
    BifEntry { module: "math", function: "pow", arity: 2, ffi_symbol: Some("nera_rt_math_pow"), param_names: Some(&["X", "Y"]), spec: Some("-spec pow(X :: number(), Y :: number()) -> float().") },
    BifEntry { module: "erlang", function: "round", arity: 1, ffi_symbol: Some("nera_rt_round"), param_names: None, spec: Some("-spec round(Number) -> integer().") },
    BifEntry { module: "erlang", function: "trunc", arity: 1, ffi_symbol: Some("nera_rt_trunc"), param_names: None, spec: Some("-spec trunc(Number) -> integer().") },
    BifEntry { module: "erlang", function: "float", arity: 1, ffi_symbol: Some("nera_rt_float"), param_names: None, spec: Some("-spec float(Number) -> float().") },
    BifEntry { module: "erlang", function: "ceil",  arity: 1, ffi_symbol: Some("nera_rt_ceil"),  param_names: None, spec: Some("-spec ceil(Number) -> integer().") },
    BifEntry { module: "erlang", function: "floor", arity: 1, ffi_symbol: Some("nera_rt_floor"), param_names: None, spec: Some("-spec floor(Number) -> integer().") },
    BifEntry { module: "erlang", function: "max",   arity: 2, ffi_symbol: Some("nera_rt_max"), param_names: Some(&["First", "Second"]), spec: Some("-spec max(Term1, Term2) -> Maximum.") },
    BifEntry { module: "erlang", function: "min",   arity: 2, ffi_symbol: Some("nera_rt_min"), param_names: Some(&["First", "Second"]), spec: Some("-spec min(Term1, Term2) -> Minimum.") },

    // -------------------------------------------------------------------------
    // erlang module — list operations
    // -------------------------------------------------------------------------
    BifEntry { module: "erlang", function: "length", arity: 1, ffi_symbol: Some("nera_rt_length"), param_names: None, spec: Some("-spec length(List) -> non_neg_integer().") },
    BifEntry { module: "erlang", function: "hd",     arity: 1, ffi_symbol: Some("nera_rt_hd"),     param_names: None, spec: Some("-spec hd(List) -> term().") },
    BifEntry { module: "erlang", function: "tl",     arity: 1, ffi_symbol: Some("nera_rt_tl"),     param_names: None, spec: Some("-spec tl(List) -> term().") },

    // -------------------------------------------------------------------------
    // erlang module — tuple operations
    // -------------------------------------------------------------------------
    BifEntry { module: "erlang", function: "element",    arity: 2, ffi_symbol: Some("nera_rt_element"),    param_names: Some(&["N", "Tuple"]), spec: Some("-spec element(N, Tuple) -> term() when N :: pos_integer().") },
    BifEntry { module: "erlang", function: "tuple_size", arity: 1, ffi_symbol: Some("nera_rt_tuple_size"), param_names: None, spec: Some("-spec tuple_size(Tuple) -> non_neg_integer().") },
    BifEntry { module: "erlang", function: "setelement", arity: 3, ffi_symbol: Some("nera_rt_setelement"), param_names: Some(&["Index", "Tuple", "Value"]), spec: Some("-spec setelement(Index, Tuple1, Value) -> Tuple2 when Index :: pos_integer().") },
    BifEntry { module: "erlang", function: "make_tuple", arity: 2, ffi_symbol: Some("nera_rt_make_tuple_2"), param_names: Some(&["Arity", "InitialValue"]), spec: Some("-spec make_tuple(Arity, InitialValue) -> tuple() when Arity :: non_neg_integer().") },
    BifEntry { module: "erlang", function: "make_tuple", arity: 3, ffi_symbol: Some("nera_rt_make_tuple_3"), param_names: Some(&["Arity", "DefaultValue", "InitList"]), spec: Some("-spec make_tuple(Arity, DefaultValue, InitList) -> tuple().") },
    BifEntry { module: "erlang", function: "append_element", arity: 2, ffi_symbol: Some("nera_rt_append_element"), param_names: Some(&["Tuple", "Element"]), spec: Some("-spec append_element(Tuple1, Term) -> Tuple2.") },

    // -------------------------------------------------------------------------
    // erlang module — type checks
    // -------------------------------------------------------------------------
    BifEntry { module: "erlang", function: "is_integer",   arity: 1, ffi_symbol: Some("nera_rt_is_integer"),   param_names: None, spec: Some("-spec is_integer(Term) -> boolean().") },
    BifEntry { module: "erlang", function: "is_float",     arity: 1, ffi_symbol: Some("nera_rt_is_float"),     param_names: None, spec: Some("-spec is_float(Term) -> boolean().") },
    BifEntry { module: "erlang", function: "is_number",    arity: 1, ffi_symbol: Some("nera_rt_is_number"),    param_names: None, spec: Some("-spec is_number(Term) -> boolean().") },
    BifEntry { module: "erlang", function: "is_atom",      arity: 1, ffi_symbol: Some("nera_rt_is_atom"),      param_names: None, spec: Some("-spec is_atom(Term) -> boolean().") },
    BifEntry { module: "erlang", function: "is_list",      arity: 1, ffi_symbol: Some("nera_rt_is_list"),      param_names: None, spec: Some("-spec is_list(Term) -> boolean().") },
    BifEntry { module: "erlang", function: "is_tuple",     arity: 1, ffi_symbol: Some("nera_rt_is_tuple"),     param_names: None, spec: Some("-spec is_tuple(Term) -> boolean().") },
    BifEntry { module: "erlang", function: "is_pid",       arity: 1, ffi_symbol: Some("nera_rt_is_pid"),       param_names: None, spec: Some("-spec is_pid(Term) -> boolean().") },
    BifEntry { module: "erlang", function: "is_binary",    arity: 1, ffi_symbol: Some("nera_rt_is_binary"),    param_names: None, spec: Some("-spec is_binary(Term) -> boolean().") },
    BifEntry { module: "erlang", function: "is_bitstring", arity: 1, ffi_symbol: Some("nera_rt_is_bitstring"), param_names: None, spec: Some("-spec is_bitstring(Term) -> boolean().") },
    BifEntry { module: "erlang", function: "is_map",       arity: 1, ffi_symbol: Some("nera_rt_is_map"),       param_names: None, spec: Some("-spec is_map(Term) -> boolean().") },
    BifEntry { module: "erlang", function: "is_function",  arity: 1, ffi_symbol: Some("nera_rt_is_function"),  param_names: None, spec: Some("-spec is_function(Term) -> boolean().") },
    BifEntry { module: "erlang", function: "is_function",  arity: 2, ffi_symbol: Some("nera_rt_is_function2"), param_names: Some(&["Term", "Arity"]), spec: Some("-spec is_function(Term, Arity) -> boolean().") },
    BifEntry { module: "erlang", function: "is_reference", arity: 1, ffi_symbol: Some("nera_rt_is_reference"), param_names: None, spec: Some("-spec is_reference(Term) -> boolean().") },
    BifEntry { module: "erlang", function: "is_boolean",   arity: 1, ffi_symbol: Some("nera_rt_is_boolean"),   param_names: None, spec: Some("-spec is_boolean(Term) -> boolean().") },

    // -------------------------------------------------------------------------
    // erlang module — conversions
    // -------------------------------------------------------------------------
    BifEntry { module: "erlang", function: "atom_to_list",    arity: 1, ffi_symbol: Some("nera_rt_atom_to_list"),    param_names: None, spec: Some("-spec atom_to_list(Atom) -> string().") },
    BifEntry { module: "erlang", function: "list_to_atom",    arity: 1, ffi_symbol: Some("nera_rt_list_to_atom"),    param_names: None, spec: Some("-spec list_to_atom(String) -> atom().") },
    BifEntry { module: "erlang", function: "integer_to_list", arity: 1, ffi_symbol: Some("nera_rt_integer_to_list"), param_names: None, spec: Some("-spec integer_to_list(Integer) -> string().") },
    BifEntry { module: "erlang", function: "integer_to_list", arity: 2, ffi_symbol: Some("nera_rt_integer_to_list_2"), param_names: Some(&["Integer", "Base"]), spec: Some("-spec integer_to_list(Integer, Base) -> string().") },
    BifEntry { module: "erlang", function: "list_to_integer", arity: 2, ffi_symbol: Some("nera_rt_list_to_integer_2"), param_names: None, spec: Some("-spec list_to_integer(String, Base) -> integer() when Base :: 2..36.") },
    BifEntry { module: "erlang", function: "list_to_integer", arity: 1, ffi_symbol: Some("nera_rt_list_to_integer"), param_names: None, spec: Some("-spec list_to_integer(String) -> integer().") },
    BifEntry { module: "erlang", function: "float_to_list",   arity: 1, ffi_symbol: Some("nera_rt_float_to_list"),   param_names: None, spec: Some("-spec float_to_list(Float) -> string().") },
    BifEntry { module: "erlang", function: "list_to_tuple",      arity: 1, ffi_symbol: Some("nera_rt_list_to_tuple"),      param_names: None, spec: Some("-spec list_to_tuple(List) -> tuple().") },
    BifEntry { module: "erlang", function: "tuple_to_list",      arity: 1, ffi_symbol: Some("nera_rt_tuple_to_list"),      param_names: None, spec: Some("-spec tuple_to_list(Tuple) -> [term()].") },
    BifEntry { module: "erlang", function: "integer_to_binary",  arity: 1, ffi_symbol: Some("nera_rt_integer_to_binary"),  param_names: None, spec: Some("-spec integer_to_binary(Integer) -> binary().") },
    BifEntry { module: "erlang", function: "integer_to_binary",  arity: 2, ffi_symbol: Some("nera_rt_integer_to_binary_2"), param_names: Some(&["Integer", "Base"]), spec: Some("-spec integer_to_binary(Integer, Base) -> binary().") },
    BifEntry { module: "erlang", function: "binary_to_integer", arity: 2, ffi_symbol: Some("nera_rt_binary_to_integer_2"), param_names: None, spec: Some("-spec binary_to_integer(Binary, Base) -> integer().") },
    BifEntry { module: "erlang", function: "binary_to_integer",  arity: 1, ffi_symbol: Some("nera_rt_binary_to_integer"),  param_names: None, spec: Some("-spec binary_to_integer(Binary) -> integer().") },
    BifEntry { module: "erlang", function: "binary_to_float",    arity: 1, ffi_symbol: Some("nera_rt_binary_to_float"),    param_names: None, spec: Some("-spec binary_to_float(Binary) -> float().") },
    BifEntry { module: "erlang", function: "list_to_binary",     arity: 1, ffi_symbol: Some("nera_rt_list_to_binary"),     param_names: None, spec: Some("-spec list_to_binary(IoList) -> binary().") },
    BifEntry { module: "erlang", function: "bitstring_to_list", arity: 1, ffi_symbol: Some("nera_rt_bitstring_to_list"), param_names: None, spec: Some("-spec bitstring_to_list(Bitstring) -> [byte() | bitstring()].") },
    BifEntry { module: "erlang", function: "list_to_bitstring", arity: 1, ffi_symbol: Some("nera_rt_list_to_bitstring"), param_names: None, spec: Some("-spec list_to_bitstring(BitstringList) -> bitstring().") },
    BifEntry { module: "crypto", function: "hash", arity: 2, ffi_symbol: Some("nera_rt_crypto_hash"), param_names: Some(&["Type", "Data"]), spec: Some("-spec hash(Type, Data) -> binary().") },
    BifEntry { module: "crypto", function: "strong_rand_bytes", arity: 1, ffi_symbol: Some("nera_rt_crypto_strong_rand_bytes"), param_names: Some(&["N"]), spec: Some("-spec strong_rand_bytes(N) -> binary().") },
    BifEntry { module: "erlang", function: "binary_to_list",     arity: 1, ffi_symbol: Some("nera_rt_binary_to_list"),     param_names: None, spec: Some("-spec binary_to_list(Binary) -> [byte()].") },
    BifEntry { module: "erlang", function: "iolist_size", arity: 1, ffi_symbol: Some("nera_rt_iolist_size"), param_names: None, spec: Some("-spec iolist_size(IoListOrBinary) -> non_neg_integer().") },
    BifEntry { module: "erlang", function: "iolist_to_binary",   arity: 1, ffi_symbol: Some("nera_rt_iolist_to_binary"),   param_names: None, spec: Some("-spec iolist_to_binary(IoListOrBinary) -> binary().") },
    BifEntry { module: "erlang", function: "atom_to_binary",     arity: 1, ffi_symbol: Some("nera_rt_atom_to_binary"),     param_names: None, spec: Some("-spec atom_to_binary(Atom) -> binary().") },
    BifEntry { module: "erlang", function: "atom_to_binary",     arity: 2, ffi_symbol: Some("nera_rt_atom_to_binary_2"),   param_names: Some(&["Atom", "Encoding"]), spec: Some("-spec atom_to_binary(Atom, Encoding) -> binary().") },
    BifEntry { module: "erlang", function: "float_to_binary",    arity: 1, ffi_symbol: Some("nera_rt_float_to_binary"),    param_names: None, spec: Some("-spec float_to_binary(Float) -> binary().") },
    BifEntry { module: "erlang", function: "float_to_binary",    arity: 2, ffi_symbol: Some("nera_rt_float_to_binary_2"),  param_names: Some(&["Float", "Options"]), spec: Some("-spec float_to_binary(Float, Options) -> binary().") },
    BifEntry { module: "erlang", function: "binary_to_atom",     arity: 1, ffi_symbol: Some("nera_rt_binary_to_atom"),     param_names: None, spec: Some("-spec binary_to_atom(Binary) -> atom().") },
    BifEntry { module: "erlang", function: "binary_to_atom",     arity: 2, ffi_symbol: Some("nera_rt_binary_to_atom_2"),   param_names: Some(&["Binary", "Encoding"]), spec: Some("-spec binary_to_atom(Binary, Encoding) -> atom().") },
    BifEntry { module: "erlang", function: "list_to_existing_atom", arity: 1, ffi_symbol: Some("nera_rt_list_to_existing_atom"), param_names: None, spec: Some("-spec list_to_existing_atom(String) -> atom().") },
    BifEntry { module: "erlang", function: "binary_to_existing_atom", arity: 1, ffi_symbol: Some("nera_rt_binary_to_existing_atom_1"), param_names: None, spec: Some("-spec binary_to_existing_atom(Binary) -> atom().") },
    BifEntry { module: "erlang", function: "binary_to_existing_atom", arity: 2, ffi_symbol: Some("nera_rt_binary_to_existing_atom"), param_names: Some(&["Binary", "Encoding"]), spec: Some("-spec binary_to_existing_atom(Binary, Encoding) -> atom().") },
    BifEntry { module: "erlang", function: "is_process_alive",   arity: 1, ffi_symbol: Some("nera_rt_is_process_alive"),   param_names: None, spec: Some("-spec is_process_alive(Pid) -> boolean().") },
    BifEntry { module: "erlang", function: "exit",               arity: 2, ffi_symbol: Some("nera_rt_exit_2"),             param_names: Some(&["Pid", "Reason"]), spec: Some("-spec exit(Pid, Reason) -> true.") },
    BifEntry { module: "erlang", function: "term_to_binary",     arity: 1, ffi_symbol: Some("nera_rt_term_to_binary"),     param_names: None, spec: Some("-spec term_to_binary(Term) -> binary().") },
    BifEntry { module: "erlang", function: "binary_to_term",     arity: 1, ffi_symbol: Some("nera_rt_binary_to_term"),     param_names: None, spec: Some("-spec binary_to_term(Binary) -> term().") },

    // -------------------------------------------------------------------------
    // erlang module — binary/map size
    // -------------------------------------------------------------------------
    BifEntry { module: "erlang", function: "size",         arity: 1, ffi_symbol: Some("nera_rt_size"),              param_names: None, spec: Some("-spec size(Item) -> non_neg_integer() when Item :: tuple() | binary().") },
    BifEntry { module: "erlang", function: "phash2",       arity: 1, ffi_symbol: Some("nera_rt_phash2"),            param_names: None, spec: Some("-spec phash2(Term) -> non_neg_integer().") },
    BifEntry { module: "erlang", function: "phash2",       arity: 2, ffi_symbol: Some("nera_rt_phash2_2"),          param_names: Some(&["Term", "Range"]), spec: Some("-spec phash2(Term, Range) -> non_neg_integer() when Range :: 1..4294967296.") },
    BifEntry { module: "erlang", function: "byte_size",    arity: 1, ffi_symbol: Some("nera_rt_binary_byte_size"), param_names: None, spec: Some("-spec byte_size(Bitstring) -> non_neg_integer().") },
    BifEntry { module: "erlang", function: "bit_size",     arity: 1, ffi_symbol: Some("nera_rt_binary_size"),      param_names: None, spec: Some("-spec bit_size(Bitstring) -> non_neg_integer().") },
    BifEntry { module: "erlang", function: "binary_part",  arity: 3, ffi_symbol: Some("nera_rt_binary_part"),      param_names: Some(&["Subject", "Start", "Length"]), spec: Some("-spec binary_part(Subject, Start, Length) -> binary().") },
    BifEntry { module: "erlang", function: "map_size",  arity: 1, ffi_symbol: Some("nera_rt_map_size"),         param_names: None, spec: Some("-spec map_size(Map) -> non_neg_integer().") },
    BifEntry { module: "erlang", function: "map_get", arity: 2, ffi_symbol: Some("nera_rt_map_get"), param_names: Some(&["Key", "Map"]), spec: Some("-spec map_get(Key, Map) -> Value.") },
    BifEntry { module: "erlang", function: "is_map_key", arity: 2, ffi_symbol: Some("nera_rt_map_is_key"), param_names: Some(&["Key", "Map"]), spec: Some("-spec is_map_key(Key, Map) -> boolean().") },
    BifEntry { module: "erlang", function: "garbage_collect", arity: 0, ffi_symbol: Some("nera_rt_garbage_collect"), param_names: None, spec: Some("-spec garbage_collect() -> true.") },

    // -------------------------------------------------------------------------
    // erlang module — process management
    // -------------------------------------------------------------------------
    BifEntry { module: "erlang", function: "group_leader", arity: 0, ffi_symbol: Some("nera_rt_group_leader"), param_names: None, spec: Some("-spec group_leader() -> pid().") },
    BifEntry { module: "erlang", function: "group_leader", arity: 2, ffi_symbol: Some("nera_rt_group_leader_2"), param_names: None, spec: Some("-spec group_leader(GroupLeader, Pid) -> true when GroupLeader :: pid(), Pid :: pid().") },
    BifEntry { module: "erlang", function: "statistics", arity: 1, ffi_symbol: Some("nera_rt_statistics"), param_names: None, spec: Some("-spec statistics(wall_clock) -> {non_neg_integer(), non_neg_integer()}.") },
    BifEntry { module: "erlang", function: "fun_info", arity: 2, ffi_symbol: Some("nera_rt_fun_info_2"), param_names: None, spec: Some("-spec fun_info(Fun, Item) -> {Item, Value} when Fun :: function(), Item :: module | name | arity, Value :: atom() | non_neg_integer().") },
    BifEntry { module: "erlang", function: "fun_info_mfa", arity: 1, ffi_symbol: Some("nera_rt_fun_info_mfa"), param_names: None, spec: Some("-spec fun_info_mfa(Fun) -> {module(), atom(), arity()} when Fun :: function().") },
    BifEntry { module: "erlang", function: "self",       arity: 0, ffi_symbol: Some("nera_rt_get_self"),         param_names: None, spec: Some("-spec self() -> pid().") },
    BifEntry { module: "erlang", function: "spawn",      arity: 1, ffi_symbol: Some("nera_rt_spawn_closure"),    param_names: None, spec: Some("-spec spawn(Fun) -> pid().") },
    BifEntry { module: "erlang", function: "spawn",      arity: 3, ffi_symbol: Some("nera_rt_spawn_process"),    param_names: Some(&["Module", "Function", "Args"]), spec: Some("-spec spawn(Module, Function, Args) -> pid().") },
    BifEntry { module: "erlang", function: "spawn_link",    arity: 1, ffi_symbol: Some("nera_rt_spawn_link_closure"),    param_names: None, spec: Some("-spec spawn_link(Fun) -> pid().") },
    BifEntry { module: "erlang", function: "spawn_link",    arity: 3, ffi_symbol: Some("nera_rt_spawn_link"),         param_names: Some(&["Module", "Function", "Args"]), spec: Some("-spec spawn_link(Module, Function, Args) -> pid().") },
    BifEntry { module: "erlang", function: "spawn_monitor", arity: 1, ffi_symbol: Some("nera_rt_spawn_monitor_closure"), param_names: None, spec: Some("-spec spawn_monitor(Fun) -> {pid(), reference()}.") },
    BifEntry { module: "erlang", function: "spawn_monitor", arity: 3, ffi_symbol: Some("nera_rt_spawn_monitor"),      param_names: Some(&["Module", "Function", "Args"]), spec: Some("-spec spawn_monitor(Module, Function, Args) -> {pid(), reference()}.") },
    BifEntry { module: "erlang", function: "make_ref",   arity: 0, ffi_symbol: Some("nera_rt_make_ref"),         param_names: None, spec: Some("-spec make_ref() -> reference().") },
    BifEntry { module: "erlang", function: "get_stacktrace", arity: 0, ffi_symbol: Some("nera_rt_get_stacktrace"), param_names: None, spec: Some("-spec get_stacktrace() -> [stack_item()].") },
    BifEntry { module: "erlang", function: "node",       arity: 0, ffi_symbol: Some("nera_rt_node"),             param_names: None, spec: Some("-spec node() -> node().") },
    BifEntry { module: "erlang", function: "node",       arity: 1, ffi_symbol: Some("nera_rt_node_1"),           param_names: None, spec: Some("-spec node(Arg) -> node() when Arg :: pid() | reference().") },

    // -------------------------------------------------------------------------
    // erlang module — time
    // -------------------------------------------------------------------------
    BifEntry { module: "erlang", function: "monotonic_time", arity: 0, ffi_symbol: Some("nera_rt_monotonic_time"),   param_names: None, spec: Some("-spec monotonic_time() -> integer().") },
    BifEntry { module: "erlang", function: "monotonic_time", arity: 1, ffi_symbol: Some("nera_rt_monotonic_time_1"), param_names: None, spec: Some("-spec monotonic_time(Unit) -> integer() when Unit :: time_unit().") },
    BifEntry { module: "erlang", function: "universaltime", arity: 0, ffi_symbol: Some("nera_rt_universaltime"), param_names: None, spec: Some("-spec universaltime() -> calendar:datetime().") },
    BifEntry { module: "erlang", function: "system_time",    arity: 1, ffi_symbol: Some("nera_rt_system_time_1"),    param_names: None, spec: Some("-spec system_time(Unit) -> integer() when Unit :: time_unit().") },
    BifEntry { module: "erlang", function: "send_after",     arity: 3, ffi_symbol: Some("nera_rt_send_after"),       param_names: Some(&["Time", "Dest", "Msg"]), spec: Some("-spec send_after(Time, Dest, Msg) -> TimerRef when Time :: non_neg_integer().") },

    // -------------------------------------------------------------------------
    // erlang module — process registration
    // -------------------------------------------------------------------------
    BifEntry { module: "erlang", function: "register",   arity: 2, ffi_symbol: Some("nera_rt_register"),   param_names: Some(&["Name", "Pid"]), spec: Some("-spec register(RegName, PidOrPort) -> true.") },
    BifEntry { module: "erlang", function: "whereis",    arity: 1, ffi_symbol: Some("nera_rt_whereis"),    param_names: None, spec: Some("-spec whereis(RegName) -> pid() | port() | undefined.") },
    BifEntry { module: "erlang", function: "unregister", arity: 1, ffi_symbol: Some("nera_rt_unregister"), param_names: None, spec: Some("-spec unregister(RegName) -> true.") },
    BifEntry { module: "erlang", function: "registered", arity: 0, ffi_symbol: Some("nera_rt_registered"), param_names: None, spec: Some("-spec registered() -> [RegName :: atom()].") },

    // -------------------------------------------------------------------------
    // erlang module — process introspection
    // -------------------------------------------------------------------------
    BifEntry { module: "erlang", function: "process_info", arity: 1, ffi_symbol: Some("nera_rt_process_info_1"), param_names: None, spec: Some("-spec process_info(Pid) -> Info | undefined.") },
    BifEntry { module: "erlang", function: "process_info", arity: 2, ffi_symbol: Some("nera_rt_process_info"), param_names: Some(&["Pid", "Item"]), spec: Some("-spec process_info(Pid, Item) -> InfoTuple | [] | undefined.") },

    // -------------------------------------------------------------------------
    // erlang module — process linking & monitoring
    // -------------------------------------------------------------------------
    BifEntry { module: "erlang", function: "link",         arity: 1, ffi_symbol: Some("nera_rt_link"),         param_names: None, spec: Some("-spec link(PidOrPort) -> true.") },
    BifEntry { module: "erlang", function: "unlink",       arity: 1, ffi_symbol: Some("nera_rt_unlink"),       param_names: None, spec: Some("-spec unlink(Id) -> true.") },
    BifEntry { module: "erlang", function: "monitor",      arity: 2, ffi_symbol: Some("nera_rt_monitor"),      param_names: Some(&["Type", "Item"]), spec: Some("-spec monitor(Type, Item) -> MonitorRef when Type :: process | port.") },
    BifEntry { module: "erlang", function: "demonitor",    arity: 1, ffi_symbol: Some("nera_rt_demonitor"),    param_names: None, spec: Some("-spec demonitor(MonitorRef) -> true.") },
    BifEntry { module: "erlang", function: "demonitor",    arity: 2, ffi_symbol: Some("nera_rt_demonitor_2"),  param_names: Some(&["MonitorRef", "OptionList"]), spec: Some("-spec demonitor(MonitorRef, OptionList) -> boolean().") },
    BifEntry { module: "erlang", function: "process_flag", arity: 2, ffi_symbol: Some("nera_rt_process_flag"), param_names: Some(&["Flag", "Value"]), spec: Some("-spec process_flag(Flag, Value) -> OldValue.") },
    BifEntry { module: "erlang", function: "hibernate",    arity: 3, ffi_symbol: Some("nera_rt_hibernate"),    param_names: Some(&["Module", "Function", "Args"]), spec: Some("-spec hibernate(Module, Function, Args) -> no_return().") },
    BifEntry { module: "erlang", function: "spawn_opt",    arity: 2, ffi_symbol: Some("nera_rt_spawn_opt_closure"), param_names: Some(&["Fun", "Options"]), spec: Some("-spec spawn_opt(Fun, Options) -> pid() | {pid(), reference()}.") },
    BifEntry { module: "erlang", function: "spawn_opt",    arity: 4, ffi_symbol: Some("nera_rt_spawn_opt"),         param_names: Some(&["Module", "Function", "Args", "Options"]), spec: Some("-spec spawn_opt(Module, Function, Args, Options) -> pid() | {pid(), reference()}.") },

    // -------------------------------------------------------------------------
    // erlang module — process dictionary
    // -------------------------------------------------------------------------
    BifEntry { module: "os", function: "getenv", arity: 1, ffi_symbol: Some("nera_rt_os_getenv"), param_names: None, spec: Some("-spec getenv(Name) -> string() | false.") },
    BifEntry { module: "erlang", function: "unique_integer", arity: 0, ffi_symbol: Some("nera_rt_unique_integer"), param_names: None, spec: Some("-spec unique_integer() -> integer().") },
    BifEntry { module: "erlang", function: "put",      arity: 2, ffi_symbol: Some("nera_rt_put"),      param_names: Some(&["Key", "Value"]), spec: Some("-spec put(Key, Val) -> OldVal | undefined.") },
    BifEntry { module: "erlang", function: "get",      arity: 0, ffi_symbol: Some("nera_rt_get_all"), param_names: None, spec: Some("-spec get() -> [{Key, Value}].") },
    BifEntry { module: "erlang", function: "get",      arity: 1, ffi_symbol: Some("nera_rt_get"),      param_names: None, spec: Some("-spec get(Key) -> Val | undefined.") },
    BifEntry { module: "erlang", function: "erase",    arity: 1, ffi_symbol: Some("nera_rt_erase"),    param_names: None, spec: Some("-spec erase(Key) -> Val | undefined.") },
    BifEntry { module: "erlang", function: "get_keys", arity: 0, ffi_symbol: Some("nera_rt_get_keys"), param_names: None, spec: Some("-spec get_keys() -> [Key].") },
    BifEntry { module: "erlang", function: "get_keys", arity: 1, ffi_symbol: Some("nera_rt_get_keys_1"), param_names: Some(&["Value"]), spec: Some("-spec get_keys(Value) -> [Key].") },

    // -------------------------------------------------------------------------
    // erlang module — exceptions
    // -------------------------------------------------------------------------
    BifEntry { module: "erlang", function: "throw", arity: 1, ffi_symbol: Some("nera_rt_throw"), param_names: None, spec: Some("-spec throw(Any) -> no_return().") },
    BifEntry { module: "erlang", function: "error", arity: 1, ffi_symbol: Some("nera_rt_error"), param_names: None, spec: Some("-spec error(Reason) -> no_return().") },
    BifEntry { module: "erlang", function: "error", arity: 2, ffi_symbol: Some("nera_rt_error_2"), param_names: Some(&["Reason", "Args"]), spec: Some("-spec error(Reason, Args) -> no_return().") },
    BifEntry { module: "erlang", function: "exit",  arity: 1, ffi_symbol: Some("nera_rt_exit"),  param_names: None, spec: Some("-spec exit(Reason) -> no_return().") },
    BifEntry { module: "erlang", function: "raise", arity: 3, ffi_symbol: Some("nera_rt_raise"), param_names: Some(&["Class", "Reason", "Stacktrace"]), spec: Some("-spec raise(Class, Reason, Stacktrace) -> no_return() when Class :: error | exit | throw.") },
    BifEntry { module: "erlang", function: "halt",  arity: 0, ffi_symbol: Some("nera_rt_halt_0"), param_names: None, spec: Some("-spec halt() -> no_return().") },
    BifEntry { module: "erlang", function: "halt",  arity: 1, ffi_symbol: Some("nera_rt_halt_1"), param_names: None, spec: Some("-spec halt(Status) -> no_return().") },

    // -------------------------------------------------------------------------
    // erlang module — meta-call
    // -------------------------------------------------------------------------
    BifEntry { module: "erlang", function: "apply", arity: 2, ffi_symbol: None, param_names: Some(&["Fun", "Args"]), spec: Some("-spec apply(Fun, Args) -> term().") },
    BifEntry { module: "erlang", function: "apply", arity: 3, ffi_symbol: None, param_names: Some(&["Module", "Function", "Args"]), spec: Some("-spec apply(Module, Function, Args) -> term().") },

    // -------------------------------------------------------------------------
    // io module
    // -------------------------------------------------------------------------
    BifEntry { module: "unicode", function: "characters_to_binary", arity: 1, ffi_symbol: Some("nera_rt_unicode_characters_to_binary"), param_names: None, spec: Some("-spec characters_to_binary(Data) -> binary() | {error | incomplete, binary(), term()} when Data :: term().") },
    BifEntry { module: "unicode", function: "characters_to_binary", arity: 2, ffi_symbol: Some("nera_rt_unicode_characters_to_binary_2"), param_names: Some(&["Data", "InEncoding"]), spec: Some("-spec characters_to_binary(Data, InEncoding) -> binary() | {error | incomplete, binary(), term()} when Data :: term(), InEncoding :: unicode:encoding().") },
    BifEntry { module: "unicode", function: "characters_to_binary", arity: 3, ffi_symbol: Some("nera_rt_unicode_characters_to_binary_3"), param_names: Some(&["Data", "InEncoding", "OutEncoding"]), spec: Some("-spec characters_to_binary(Data, InEncoding, OutEncoding) -> binary() | {error | incomplete, binary(), term()} when Data :: term(), InEncoding :: unicode:encoding(), OutEncoding :: unicode:encoding().") },
    BifEntry { module: "unicode", function: "characters_to_list", arity: 1, ffi_symbol: Some("nera_rt_unicode_characters_to_list"), param_names: None, spec: Some("-spec characters_to_list(Data) -> [char()] | {error | incomplete, [char()], term()} when Data :: term().") },
    BifEntry { module: "unicode", function: "characters_to_list", arity: 2, ffi_symbol: Some("nera_rt_unicode_characters_to_list_2"), param_names: Some(&["Data", "InEncoding"]), spec: Some("-spec characters_to_list(Data, InEncoding) -> [char()] | {error | incomplete, [char()], term()} when Data :: term(), InEncoding :: unicode:encoding().") },
    BifEntry { module: "io", function: "put_chars", arity: 1, ffi_symbol: Some("nera_rt_io_put_chars"), param_names: None, spec: Some("-spec put_chars(Chars) -> ok when Chars :: term().") },
    BifEntry { module: "io", function: "fwrite", arity: 2, ffi_symbol: Some("nera_rt_io_format"), param_names: None, spec: Some("-spec fwrite(Format, Data) -> ok when Format :: string(), Data :: [term()].") },
    BifEntry { module: "io_lib", function: "fwrite", arity: 2, ffi_symbol: Some("nera_rt_io_lib_format"), param_names: None, spec: Some("-spec fwrite(Format, Data) -> chars() when Format :: string(), Data :: [term()].") },
    BifEntry { module: "io", function: "format", arity: 1, ffi_symbol: Some("nera_rt_io_format_1"), param_names: None, spec: Some("-spec format(Format) -> ok.") },
    BifEntry { module: "io", function: "format", arity: 2, ffi_symbol: Some("nera_rt_io_format"),   param_names: Some(&["Format", "Data"]), spec: Some("-spec format(Format, Data) -> ok.") },

    // -------------------------------------------------------------------------
    // io_lib module
    // -------------------------------------------------------------------------
    BifEntry { module: "io_lib", function: "format", arity: 2, ffi_symbol: Some("nera_rt_io_lib_format"), param_names: Some(&["Format", "Data"]), spec: Some("-spec format(Format, Data) -> chars().") },

    // -------------------------------------------------------------------------
    // error_logger module
    // -------------------------------------------------------------------------
    BifEntry { module: "error_logger", function: "info_msg",    arity: 2, ffi_symbol: Some("nera_rt_error_logger_info_msg"),    param_names: Some(&["Format", "Data"]), spec: Some("-spec info_msg(Format, Data) -> ok.") },
    BifEntry { module: "error_logger", function: "warning_msg", arity: 2, ffi_symbol: Some("nera_rt_error_logger_warning_msg"), param_names: Some(&["Format", "Data"]), spec: Some("-spec warning_msg(Format, Data) -> ok.") },
    BifEntry { module: "error_logger", function: "error_msg",   arity: 2, ffi_symbol: Some("nera_rt_error_logger_error_msg"),   param_names: Some(&["Format", "Data"]), spec: Some("-spec error_msg(Format, Data) -> ok.") },

    // -------------------------------------------------------------------------
    // maps module
    // -------------------------------------------------------------------------
    BifEntry { module: "maps", function: "new",       arity: 0, ffi_symbol: Some("nera_rt_map_new"),       param_names: None, spec: Some("-spec new() -> map().") },
    BifEntry { module: "maps", function: "get",       arity: 2, ffi_symbol: Some("nera_rt_map_get"),       param_names: Some(&["Key", "Map"]), spec: Some("-spec get(Key, Map) -> Value.") },
    BifEntry { module: "maps", function: "get",       arity: 3, ffi_symbol: Some("nera_rt_map_get_3"),     param_names: Some(&["Key", "Map", "Default"]), spec: Some("-spec get(Key, Map, Default) -> Value | Default.") },
    BifEntry { module: "maps", function: "find",      arity: 2, ffi_symbol: Some("nera_rt_map_find"),      param_names: Some(&["Key", "Map"]), spec: Some("-spec find(Key, Map) -> {ok, Value} | error.") },
    BifEntry { module: "maps", function: "put",       arity: 3, ffi_symbol: Some("nera_rt_map_put"),       param_names: Some(&["Key", "Value", "Map"]), spec: Some("-spec put(Key, Value, Map1) -> Map2.") },
    BifEntry { module: "maps", function: "remove",    arity: 2, ffi_symbol: Some("nera_rt_map_remove"),    param_names: Some(&["Key", "Map"]), spec: Some("-spec remove(Key, Map1) -> Map2.") },
    BifEntry { module: "maps", function: "is_key",    arity: 2, ffi_symbol: Some("nera_rt_map_is_key"),    param_names: Some(&["Key", "Map"]), spec: Some("-spec is_key(Key, Map) -> boolean().") },
    BifEntry { module: "maps", function: "keys",      arity: 1, ffi_symbol: Some("nera_rt_map_keys"),      param_names: None, spec: Some("-spec keys(Map) -> Keys when Keys :: [Key].") },
    BifEntry { module: "maps", function: "values",    arity: 1, ffi_symbol: Some("nera_rt_map_values"),    param_names: None, spec: Some("-spec values(Map) -> Values when Values :: [Value].") },
    BifEntry { module: "maps", function: "to_list",   arity: 1, ffi_symbol: Some("nera_rt_map_to_list"),   param_names: None, spec: Some("-spec to_list(Map) -> [{Key, Value}].") },
    BifEntry { module: "maps", function: "from_list", arity: 1, ffi_symbol: Some("nera_rt_map_from_list"), param_names: None, spec: Some("-spec from_list(List) -> Map when List :: [{Key, Value}].") },
    BifEntry { module: "maps", function: "from_keys", arity: 2, ffi_symbol: Some("nera_rt_map_from_keys"), param_names: Some(&["Keys", "Value"]), spec: Some("-spec from_keys(Keys, Value) -> Map.") },
    BifEntry { module: "maps", function: "size",      arity: 1, ffi_symbol: Some("nera_rt_map_size"),      param_names: None, spec: Some("-spec size(Map) -> non_neg_integer().") },
    BifEntry { module: "maps", function: "merge",     arity: 2, ffi_symbol: Some("nera_rt_map_merge"),     param_names: Some(&["Map1", "Map2"]), spec: Some("-spec merge(Map1, Map2) -> Map3.") },
    BifEntry { module: "maps", function: "update",    arity: 3, ffi_symbol: Some("nera_rt_map_update"),    param_names: Some(&["Key", "Value", "Map"]), spec: Some("-spec update(Key, Value, Map1) -> Map2.") },
    BifEntry { module: "maps", function: "take",      arity: 2, ffi_symbol: Some("nera_rt_map_take"),      param_names: Some(&["Key", "Map"]), spec: Some("-spec take(Key, Map1) -> {Value, Map2} | error.") },

    // -------------------------------------------------------------------------
    // lists module
    // -------------------------------------------------------------------------
    BifEntry { module: "lists", function: "reverse",   arity: 1, ffi_symbol: Some("nera_rt_lists_reverse"),   param_names: None, spec: Some("-spec reverse(List1) -> List2.") },
    BifEntry { module: "lists", function: "reverse",   arity: 2, ffi_symbol: Some("nera_rt_lists_reverse_2"), param_names: Some(&["List", "Tail"]), spec: Some("-spec reverse(List1, Tail) -> List2.") },
    BifEntry { module: "lists", function: "member",    arity: 2, ffi_symbol: Some("nera_rt_lists_member"),    param_names: Some(&["Elem", "List"]), spec: Some("-spec member(Elem, List) -> boolean().") },
    BifEntry { module: "lists", function: "keyfind",   arity: 3, ffi_symbol: Some("nera_rt_lists_keyfind"),   param_names: Some(&["Key", "N", "TupleList"]), spec: Some("-spec keyfind(Key, N, TupleList) -> Tuple | false.") },
    BifEntry { module: "lists", function: "keymember", arity: 3, ffi_symbol: Some("nera_rt_lists_keymember"), param_names: Some(&["Key", "N", "TupleList"]), spec: Some("-spec keymember(Key, N, TupleList) -> boolean().") },
    BifEntry { module: "lists", function: "keysearch", arity: 3, ffi_symbol: Some("nera_rt_lists_keysearch"), param_names: Some(&["Key", "N", "TupleList"]), spec: Some("-spec keysearch(Key, N, TupleList) -> {value, Tuple} | false.") },
    BifEntry { module: "lists", function: "append",    arity: 1, ffi_symbol: Some("nera_rt_lists_append"),    param_names: None, spec: Some("-spec append(ListOfLists) -> List1.") },
    BifEntry { module: "lists", function: "last",      arity: 1, ffi_symbol: Some("nera_rt_lists_last"),      param_names: None, spec: Some("-spec last(List) -> Last.") },
    BifEntry { module: "lists", function: "nth",       arity: 2, ffi_symbol: Some("nera_rt_lists_nth"),       param_names: Some(&["N", "List"]), spec: Some("-spec nth(N, List) -> Elem when N :: pos_integer().") },

    // -------------------------------------------------------------------------
    // gen_tcp module (networking)
    // -------------------------------------------------------------------------
    BifEntry { module: "gen_tcp", function: "listen", arity: 2, ffi_symbol: Some("nera_rt_gen_tcp_listen"), param_names: Some(&["Port", "Options"]), spec: Some("-spec listen(Port, Options) -> {ok, ListenSocket} | {error, Reason}.") },
    BifEntry { module: "gen_tcp", function: "accept", arity: 1, ffi_symbol: Some("nera_rt_gen_tcp_accept"), param_names: None, spec: Some("-spec accept(ListenSocket) -> {ok, Socket} | {error, Reason}.") },
    BifEntry { module: "gen_tcp", function: "recv",   arity: 2, ffi_symbol: Some("nera_rt_gen_tcp_recv"),   param_names: Some(&["Socket", "Length"]), spec: Some("-spec recv(Socket, Length) -> {ok, Packet} | {error, Reason}.") },
    BifEntry { module: "gen_tcp", function: "send",   arity: 2, ffi_symbol: Some("nera_rt_gen_tcp_send"),   param_names: Some(&["Socket", "Packet"]), spec: Some("-spec send(Socket, Packet) -> ok | {error, Reason}.") },
    BifEntry { module: "gen_tcp", function: "close",              arity: 1, ffi_symbol: Some("nera_rt_gen_tcp_close"),              param_names: None, spec: Some("-spec close(Socket) -> ok.") },
    BifEntry { module: "gen_tcp", function: "recv",               arity: 3, ffi_symbol: Some("nera_rt_gen_tcp_recv_3"),            param_names: Some(&["Socket", "Length", "Timeout"]), spec: Some("-spec recv(Socket, Length, Timeout) -> {ok, Packet} | {error, Reason}.") },
    BifEntry { module: "gen_tcp", function: "controlling_process", arity: 2, ffi_symbol: Some("nera_rt_gen_tcp_controlling_process"), param_names: Some(&["Socket", "Pid"]), spec: Some("-spec controlling_process(Socket, Pid) -> ok | {error, Reason}.") },
    BifEntry { module: "gen_tcp", function: "connect",             arity: 3, ffi_symbol: Some("nera_rt_gen_tcp_connect"),             param_names: Some(&["Address", "Port", "Options"]), spec: Some("-spec connect(Address, Port, Options) -> {ok, Socket} | {error, Reason}.") },

    // -------------------------------------------------------------------------
    // inet module (socket options & introspection)
    // -------------------------------------------------------------------------
    BifEntry { module: "inet", function: "setopts", arity: 2, ffi_symbol: Some("nera_rt_inet_setopts"), param_names: Some(&["Socket", "Options"]), spec: Some("-spec setopts(Socket, Options) -> ok | {error, Reason}.") },
    BifEntry { module: "inet", function: "port",    arity: 1, ffi_symbol: Some("nera_rt_inet_port"),    param_names: None, spec: Some("-spec port(Socket) -> {ok, Port} | {error, any()}.") },

    // -------------------------------------------------------------------------
    // binary module
    // -------------------------------------------------------------------------
    BifEntry { module: "binary", function: "split", arity: 2, ffi_symbol: Some("nera_rt_binary_split_2"), param_names: Some(&["Subject", "Pattern"]), spec: Some("-spec split(Subject, Pattern) -> Parts when Parts :: [binary()].") },
    BifEntry { module: "binary", function: "split", arity: 3, ffi_symbol: Some("nera_rt_binary_split_3"), param_names: Some(&["Subject", "Pattern", "Options"]), spec: Some("-spec split(Subject, Pattern, Options) -> Parts when Parts :: [binary()].") },
    BifEntry { module: "binary", function: "part",  arity: 3, ffi_symbol: Some("nera_rt_binary_part"),    param_names: Some(&["Subject", "Start", "Length"]), spec: Some("-spec part(Subject, Start, Length) -> binary().") },
    BifEntry { module: "string", function: "lowercase", arity: 1, ffi_symbol: Some("nera_rt_string_lowercase"), param_names: Some(&["String"]), spec: Some("-spec lowercase(String) -> unicode:chardata().") },
    BifEntry { module: "string", function: "slice", arity: 3, ffi_symbol: Some("nera_rt_string_slice"), param_names: Some(&["String", "Start", "Length"]), spec: Some("-spec slice(String, Start, Length) -> unicode:chardata().") },
    BifEntry { module: "string", function: "trim", arity: 3, ffi_symbol: Some("nera_rt_string_trim"), param_names: Some(&["String", "Direction", "Characters"]), spec: Some("-spec trim(String, Direction, Characters) -> unicode:chardata().") },
    BifEntry { module: "binary", function: "last", arity: 1, ffi_symbol: Some("nera_rt_binary_last"), param_names: Some(&["Subject"]), spec: Some("-spec last(Subject) -> byte().") },
    BifEntry { module: "binary", function: "longest_common_prefix", arity: 1, ffi_symbol: Some("nera_rt_binary_longest_common_prefix"), param_names: Some(&["Binaries"]), spec: Some("-spec longest_common_prefix(Binaries) -> non_neg_integer().") },
    BifEntry { module: "binary", function: "match", arity: 3, ffi_symbol: Some("nera_rt_binary_match_3"), param_names: Some(&["Subject", "Pattern", "Options"]), spec: Some("-spec match(Subject, Pattern, Options) -> {non_neg_integer(), pos_integer()} | nomatch when Options :: [{scope, {integer(), integer()}}].") },
    BifEntry { module: "binary", function: "match", arity: 2, ffi_symbol: Some("nera_rt_binary_match"), param_names: Some(&["Subject", "Pattern"]), spec: Some("-spec match(Subject, Pattern) -> {non_neg_integer(), pos_integer()} | nomatch.") },
    BifEntry { module: "binary", function: "at",    arity: 2, ffi_symbol: Some("nera_rt_binary_at"),      param_names: Some(&["Subject", "Pos"]), spec: Some("-spec at(Subject, Pos) -> byte().") },
    BifEntry { module: "binary", function: "referenced_byte_size", arity: 1, ffi_symbol: Some("nera_rt_binary_referenced_byte_size"), param_names: Some(&["Binary"]), spec: Some("-spec referenced_byte_size(Binary) -> non_neg_integer().") },
    BifEntry { module: "binary", function: "copy",  arity: 1, ffi_symbol: Some("nera_rt_binary_copy_1"),  param_names: Some(&["Subject"]), spec: Some("-spec copy(Subject) -> binary().") },
    BifEntry { module: "binary", function: "copy",  arity: 2, ffi_symbol: Some("nera_rt_binary_copy_2"),  param_names: Some(&["Subject", "N"]), spec: Some("-spec copy(Subject, N) -> binary().") },

    // -------------------------------------------------------------------------
    // erlang module — timer
    // -------------------------------------------------------------------------
    BifEntry { module: "erlang", function: "start_timer",    arity: 3, ffi_symbol: Some("nera_rt_start_timer"),    param_names: Some(&["Time", "Dest", "Msg"]), spec: Some("-spec start_timer(Time, Dest, Msg) -> TimerRef when Time :: non_neg_integer().") },
    BifEntry { module: "erlang", function: "cancel_timer",   arity: 1, ffi_symbol: Some("nera_rt_cancel_timer"),   param_names: None, spec: Some("-spec cancel_timer(TimerRef) -> Time | false when Time :: non_neg_integer().") },
    BifEntry { module: "erlang", function: "cancel_timer",   arity: 2, ffi_symbol: Some("nera_rt_cancel_timer_2"), param_names: Some(&["TimerRef", "Options"]), spec: Some("-spec cancel_timer(TimerRef, Options) -> non_neg_integer() | false | ok when TimerRef :: reference(), Options :: [{async, boolean()} | {info, boolean()}].") },

    // -------------------------------------------------------------------------
    // ets module
    // -------------------------------------------------------------------------
    BifEntry { module: "ets", function: "new",                arity: 2, ffi_symbol: Some("nera_rt_ets_new"),                param_names: Some(&["Name", "Options"]), spec: Some("-spec new(Name, Options) -> tid() | atom().") },
    BifEntry { module: "ets", function: "insert",             arity: 2, ffi_symbol: Some("nera_rt_ets_insert"),             param_names: Some(&["Table", "Objects"]), spec: Some("-spec insert(Table, ObjectOrObjects) -> true.") },
    BifEntry { module: "ets", function: "lookup",             arity: 2, ffi_symbol: Some("nera_rt_ets_lookup"),             param_names: Some(&["Table", "Key"]), spec: Some("-spec lookup(Table, Key) -> [Object].") },
    BifEntry { module: "ets", function: "delete",             arity: 2, ffi_symbol: Some("nera_rt_ets_delete"),             param_names: Some(&["Table", "Key"]), spec: Some("-spec delete(Table, Key) -> true.") },
    BifEntry { module: "ets", function: "delete",             arity: 1, ffi_symbol: Some("nera_rt_ets_delete_table"),       param_names: None, spec: Some("-spec delete(Table) -> true.") },
    BifEntry { module: "ets", function: "delete_all_objects", arity: 1, ffi_symbol: Some("nera_rt_ets_delete_all_objects"), param_names: None, spec: Some("-spec delete_all_objects(Table) -> true.") },
    BifEntry { module: "ets", function: "update_element",     arity: 3, ffi_symbol: Some("nera_rt_ets_update_element"),     param_names: Some(&["Table", "Key", "ElementSpec"]), spec: Some("-spec update_element(Table, Key, ElementSpec) -> boolean().") },
    BifEntry { module: "ets", function: "update_counter",     arity: 3, ffi_symbol: Some("nera_rt_ets_update_counter"),     param_names: Some(&["Table", "Key", "UpdateOp"]), spec: Some("-spec update_counter(Table, Key, UpdateOp) -> Result.") },
    BifEntry { module: "ets", function: "select_delete",      arity: 2, ffi_symbol: Some("nera_rt_ets_select_delete"),      param_names: Some(&["Table", "MatchSpec"]), spec: Some("-spec select_delete(Table, MatchSpec) -> NumDeleted when NumDeleted :: non_neg_integer().") },
    BifEntry { module: "ets", function: "member",             arity: 2, ffi_symbol: Some("nera_rt_ets_member"),             param_names: Some(&["Table", "Key"]), spec: Some("-spec member(Table, Key) -> boolean().") },
    BifEntry { module: "ets", function: "insert_new",         arity: 2, ffi_symbol: Some("nera_rt_ets_insert_new"),         param_names: Some(&["Table", "Objects"]), spec: Some("-spec insert_new(Table, ObjectOrObjects) -> boolean().") },
    BifEntry { module: "ets", function: "tab2list",           arity: 1, ffi_symbol: Some("nera_rt_ets_tab2list"),           param_names: None, spec: Some("-spec tab2list(Table) -> [Object].") },
    BifEntry { module: "ets", function: "first",              arity: 1, ffi_symbol: Some("nera_rt_ets_first"),              param_names: None, spec: Some("-spec first(Table) -> Key | '$end_of_table'.") },
    BifEntry { module: "ets", function: "next",               arity: 2, ffi_symbol: Some("nera_rt_ets_next"),               param_names: Some(&["Table", "Key"]), spec: Some("-spec next(Table, Key1) -> Key2 | '$end_of_table'.") },
    BifEntry { module: "ets", function: "last",               arity: 1, ffi_symbol: Some("nera_rt_ets_last"),               param_names: None, spec: Some("-spec last(Table) -> Key | '$end_of_table'.") },
    BifEntry { module: "ets", function: "prev",               arity: 2, ffi_symbol: Some("nera_rt_ets_prev"),               param_names: Some(&["Table", "Key"]), spec: Some("-spec prev(Table, Key1) -> Key2 | '$end_of_table'.") },
    BifEntry { module: "ets", function: "info",               arity: 1, ffi_symbol: Some("nera_rt_ets_info_1"),             param_names: None, spec: Some("-spec info(Table) -> InfoList | undefined.") },
    BifEntry { module: "ets", function: "info",               arity: 2, ffi_symbol: Some("nera_rt_ets_info_2"),             param_names: Some(&["Table", "Item"]), spec: Some("-spec info(Table, Item) -> Value | undefined.") },
    BifEntry { module: "ets", function: "give_away",          arity: 3, ffi_symbol: Some("nera_rt_ets_give_away"),          param_names: Some(&["Table", "Pid", "GiftData"]), spec: Some("-spec give_away(Table, Pid, GiftData) -> true.") },

    // -------------------------------------------------------------------------
    // persistent_term module (OTP 21+)
    // -------------------------------------------------------------------------
    BifEntry { module: "persistent_term", function: "put",   arity: 2, ffi_symbol: Some("nera_rt_persistent_term_put"),   param_names: Some(&["Key", "Value"]), spec: Some("-spec put(Key, Value) -> ok.") },
    BifEntry { module: "persistent_term", function: "get",   arity: 1, ffi_symbol: Some("nera_rt_persistent_term_get"),   param_names: None, spec: Some("-spec get(Key) -> Value.") },
    BifEntry { module: "persistent_term", function: "get",   arity: 2, ffi_symbol: Some("nera_rt_persistent_term_get_2"), param_names: Some(&["Key", "Default"]), spec: Some("-spec get(Key, Default) -> Value.") },
    BifEntry { module: "persistent_term", function: "erase", arity: 1, ffi_symbol: Some("nera_rt_persistent_term_erase"), param_names: None, spec: Some("-spec erase(Key) -> boolean().") },
    BifEntry { module: "persistent_term", function: "info",  arity: 0, ffi_symbol: Some("nera_rt_persistent_term_info"),  param_names: None, spec: Some("-spec info() -> InfoList when InfoList :: [{count, Count} | {memory, Bytes}].") },

    // -------------------------------------------------------------------------
    // counters module (OTP 22+)
    // -------------------------------------------------------------------------
    BifEntry { module: "counters", function: "new",  arity: 2, ffi_symbol: Some("nera_rt_counters_new"),  param_names: Some(&["Size", "Options"]), spec: Some("-spec new(Size, Opts) -> counters_ref() when Size :: pos_integer().") },
    BifEntry { module: "counters", function: "get",  arity: 2, ffi_symbol: Some("nera_rt_counters_get"),  param_names: Some(&["Ref", "Ix"]), spec: Some("-spec get(Ref, Ix) -> integer().") },
    BifEntry { module: "counters", function: "put",  arity: 3, ffi_symbol: Some("nera_rt_counters_put"),  param_names: Some(&["Ref", "Ix", "Value"]), spec: Some("-spec put(Ref, Ix, Value) -> ok.") },
    BifEntry { module: "counters", function: "add",  arity: 3, ffi_symbol: Some("nera_rt_counters_add"),  param_names: Some(&["Ref", "Ix", "Incr"]), spec: Some("-spec add(Ref, Ix, Incr) -> ok.") },
    BifEntry { module: "counters", function: "sub",  arity: 3, ffi_symbol: Some("nera_rt_counters_sub"),  param_names: Some(&["Ref", "Ix", "Decr"]), spec: Some("-spec sub(Ref, Ix, Decr) -> ok.") },
    BifEntry { module: "counters", function: "info", arity: 1, ffi_symbol: Some("nera_rt_counters_info"), param_names: None, spec: Some("-spec info(Ref) -> Info when Info :: #{size := Size, memory := Bytes}.") },

    // -------------------------------------------------------------------------
    // zlib module (streaming deflate/inflate)
    BifEntry { module: "zlib", function: "open", arity: 0, ffi_symbol: Some("nera_rt_zlib_open"), param_names: None, spec: Some("-spec open() -> zstream().") },
    BifEntry { module: "zlib", function: "close", arity: 1, ffi_symbol: Some("nera_rt_zlib_close"), param_names: Some(&["Z"]), spec: Some("-spec close(Z) -> ok.") },
    BifEntry { module: "zlib", function: "deflateInit", arity: 2, ffi_symbol: Some("nera_rt_zlib_deflate_init_2"), param_names: Some(&["Z", "Level"]), spec: Some("-spec deflateInit(Z, Level) -> ok.") },
    BifEntry { module: "zlib", function: "deflateInit", arity: 6, ffi_symbol: Some("nera_rt_zlib_deflate_init"), param_names: Some(&["Z", "Level", "Method", "WindowBits", "MemLevel", "Strategy"]), spec: Some("-spec deflateInit(Z, Level, Method, WindowBits, MemLevel, Strategy) -> ok.") },
    BifEntry { module: "zlib", function: "deflate", arity: 2, ffi_symbol: Some("nera_rt_zlib_deflate_2"), param_names: Some(&["Z", "Data"]), spec: Some("-spec deflate(Z, Data) -> iolist().") },
    BifEntry { module: "zlib", function: "deflate", arity: 3, ffi_symbol: Some("nera_rt_zlib_deflate"), param_names: Some(&["Z", "Data", "Flush"]), spec: Some("-spec deflate(Z, Data, Flush) -> iolist() when Flush :: none | sync | full | finish.") },
    BifEntry { module: "zlib", function: "deflateReset", arity: 1, ffi_symbol: Some("nera_rt_zlib_deflate_reset"), param_names: Some(&["Z"]), spec: Some("-spec deflateReset(Z) -> ok.") },
    BifEntry { module: "zlib", function: "deflateEnd", arity: 1, ffi_symbol: Some("nera_rt_zlib_deflate_end"), param_names: Some(&["Z"]), spec: Some("-spec deflateEnd(Z) -> ok.") },
    BifEntry { module: "zlib", function: "inflateInit", arity: 1, ffi_symbol: Some("nera_rt_zlib_inflate_init_1"), param_names: Some(&["Z"]), spec: Some("-spec inflateInit(Z) -> ok.") },
    BifEntry { module: "zlib", function: "inflateInit", arity: 2, ffi_symbol: Some("nera_rt_zlib_inflate_init"), param_names: Some(&["Z", "WindowBits"]), spec: Some("-spec inflateInit(Z, WindowBits) -> ok.") },
    BifEntry { module: "zlib", function: "inflate", arity: 2, ffi_symbol: Some("nera_rt_zlib_inflate"), param_names: Some(&["Z", "Data"]), spec: Some("-spec inflate(Z, Data) -> iolist().") },
    BifEntry { module: "zlib", function: "safeInflate", arity: 2, ffi_symbol: Some("nera_rt_zlib_safe_inflate"), param_names: Some(&["Z", "Data"]), spec: Some("-spec safeInflate(Z, Data) -> {continue | finished, iolist()}.") },
    BifEntry { module: "zlib", function: "inflateReset", arity: 1, ffi_symbol: Some("nera_rt_zlib_inflate_reset"), param_names: Some(&["Z"]), spec: Some("-spec inflateReset(Z) -> ok.") },
    BifEntry { module: "zlib", function: "inflateEnd", arity: 1, ffi_symbol: Some("nera_rt_zlib_inflate_end"), param_names: Some(&["Z"]), spec: Some("-spec inflateEnd(Z) -> ok.") },
    BifEntry { module: "zlib", function: "set_controlling_process", arity: 2, ffi_symbol: Some("nera_rt_zlib_set_controlling_process"), param_names: Some(&["Z", "Pid"]), spec: Some("-spec set_controlling_process(Z, Pid) -> ok.") },
    // atomics module (OTP 22+)
    // -------------------------------------------------------------------------
    BifEntry { module: "atomics", function: "new",              arity: 2, ffi_symbol: Some("nera_rt_atomics_new"),              param_names: Some(&["Arity", "Options"]), spec: Some("-spec new(Arity, Opts) -> atomics_ref() when Arity :: pos_integer().") },
    BifEntry { module: "atomics", function: "get",              arity: 2, ffi_symbol: Some("nera_rt_atomics_get"),              param_names: Some(&["Ref", "Ix"]), spec: Some("-spec get(Ref, Ix) -> integer().") },
    BifEntry { module: "atomics", function: "put",              arity: 3, ffi_symbol: Some("nera_rt_atomics_put"),              param_names: Some(&["Ref", "Ix", "Value"]), spec: Some("-spec put(Ref, Ix, Value) -> ok.") },
    BifEntry { module: "atomics", function: "add",              arity: 3, ffi_symbol: Some("nera_rt_atomics_add"),              param_names: Some(&["Ref", "Ix", "Incr"]), spec: Some("-spec add(Ref, Ix, Incr) -> ok.") },
    BifEntry { module: "atomics", function: "sub",              arity: 3, ffi_symbol: Some("nera_rt_atomics_sub"),              param_names: Some(&["Ref", "Ix", "Decr"]), spec: Some("-spec sub(Ref, Ix, Decr) -> ok.") },
    BifEntry { module: "atomics", function: "add_get",          arity: 3, ffi_symbol: Some("nera_rt_atomics_add_get"),          param_names: Some(&["Ref", "Ix", "Incr"]), spec: Some("-spec add_get(Ref, Ix, Incr) -> integer().") },
    BifEntry { module: "atomics", function: "sub_get",          arity: 3, ffi_symbol: Some("nera_rt_atomics_sub_get"),          param_names: Some(&["Ref", "Ix", "Decr"]), spec: Some("-spec sub_get(Ref, Ix, Decr) -> integer().") },
    BifEntry { module: "atomics", function: "exchange",         arity: 3, ffi_symbol: Some("nera_rt_atomics_exchange"),         param_names: Some(&["Ref", "Ix", "Desired"]), spec: Some("-spec exchange(Ref, Ix, Desired) -> integer().") },
    BifEntry { module: "atomics", function: "compare_exchange", arity: 4, ffi_symbol: Some("nera_rt_atomics_compare_exchange"), param_names: Some(&["Ref", "Ix", "Expected", "Desired"]), spec: Some("-spec compare_exchange(Ref, Ix, Expected, Desired) -> ok | integer().") },
    BifEntry { module: "atomics", function: "info",             arity: 1, ffi_symbol: Some("nera_rt_atomics_info"),             param_names: None, spec: Some("-spec info(Ref) -> Info when Info :: #{size := Size, max := Max, min := Min, memory := Bytes}.") },

    // -------------------------------------------------------------------------
    // file module (file I/O)
    // -------------------------------------------------------------------------
    BifEntry { module: "file", function: "read_file",  arity: 1, ffi_symbol: Some("nera_rt_file_read_file"),  param_names: None, spec: Some("-spec read_file(Filename) -> {ok, Binary} | {error, Reason}.") },
    BifEntry { module: "file", function: "write_file", arity: 2, ffi_symbol: Some("nera_rt_file_write_file"), param_names: Some(&["Filename", "Bytes"]), spec: Some("-spec write_file(Filename, Bytes) -> ok | {error, Reason}.") },

    // -------------------------------------------------------------------------
    // nera module — runtime diagnostics
    // -------------------------------------------------------------------------
    BifEntry { module: "nera", function: "memory_stats", arity: 0, ffi_symbol: Some("nera_rt_memory_stats"), param_names: None, spec: Some("-spec memory_stats() -> [{atom(), non_neg_integer()}].") },
    BifEntry { module: "nera", function: "version", arity: 0, ffi_symbol: Some("nera_rt_nera_version"), param_names: None, spec: Some("-spec version() -> string().") },
    BifEntry { module: "nera", function: "compile_date", arity: 0, ffi_symbol: Some("nera_rt_nera_compile_date"), param_names: None, spec: Some("-spec compile_date() -> string().") },
];

/// Look up the FFI symbol for a `module:function/arity` remote call.
///
/// Returns `Some(symbol)` if the BIF has a direct FFI implementation,
/// or `None` if it should fall through to `nera_rt_apply`.
pub fn lookup_ffi_symbol(module: &str, function: &str, arity: usize) -> Option<&'static str> {
    BIF_TABLE.iter()
        .find(|e| e.module == module && e.function == function && e.arity as usize == arity)
        .and_then(|e| e.ffi_symbol)
}

/// Check if a `module:function/arity` is a known BIF (regardless of FFI symbol).
///
/// Returns `true` if the function exists in the BIF table, even if it has no
/// direct FFI implementation (i.e. falls through to `nera_rt_apply` at runtime).
pub fn is_known_bif(module: &str, function: &str, arity: u32) -> bool {
    BIF_TABLE.iter()
        .any(|e| e.module == module && e.function == function && e.arity == arity)
}

/// Check if a function name + arity is auto-imported from the `erlang` module.
///
/// Auto-imported BIFs can be called without the `erlang:` prefix in Erlang source.
/// For example, `length(X)` is rewritten to `erlang:length(X)` during IR lowering.
pub fn is_auto_import(function: &str, arity: u32) -> bool {
    crate::otp_auto_imports::OTP_AUTO_IMPORTS.contains(&(function, arity))
}

/// Whether a native/editor metadata entry also has an OTP auto-import name.
pub fn is_auto_imported_entry(entry: &BifEntry) -> bool {
    entry.module == "erlang" && is_auto_import(entry.function, entry.arity)
}

/// Look up parameter names for a BIF call (used by LSP inlay hints).
///
/// Returns `Some(names)` if the BIF has meaningful parameter names defined,
/// or `None` if the BIF is unknown or its parameter names are intentionally
/// omitted (e.g. single-arg type checks where hints add no value).
pub fn lookup_bif_param_names(module: &str, function: &str, arity: u32) -> Option<&'static [&'static str]> {
    BIF_TABLE.iter()
        .find(|e| e.module == module && e.function == function && e.arity == arity)
        .and_then(|e| e.param_names)
}

/// Look up the Erlang-style type spec for a BIF (used by LSP hover and type checking).
///
/// Returns `Some(spec_str)` with the full `-spec ...` string, or `None` if the
/// BIF is unknown or has no spec defined yet.
pub fn lookup_bif_spec(module: &str, function: &str, arity: u32) -> Option<&'static str> {
    BIF_TABLE.iter()
        .find(|e| e.module == module && e.function == function && e.arity == arity)
        .and_then(|e| e.spec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_map_key_is_available_as_a_bif() {
        // NERA-44: cow_ws needs the auto-import and qualified runtime lookup.
        assert!(is_auto_import("is_map_key", 2));
        assert!(is_known_bif("erlang", "is_map_key", 2));
        assert_eq!(lookup_ffi_symbol("erlang", "is_map_key", 2),
                   lookup_ffi_symbol("maps", "is_key", 2));
        assert!(!is_auto_import("is_map_key", 1));
    }

    #[test]
    fn list_to_bitstring_is_available_as_a_bif() {
        // Unqualified calls and fun references resolve through the auto-import
        // to erlang:list_to_bitstring/1, which must reach the runtime join.
        assert!(is_auto_import("list_to_bitstring", 1));
        assert!(is_known_bif("erlang", "list_to_bitstring", 1));
        assert_eq!(lookup_ffi_symbol("erlang", "list_to_bitstring", 1),
                   Some("nera_rt_list_to_bitstring"));
    }

    #[test]
    fn crypto_hash_and_strong_rand_bytes_are_available_as_bifs() {
        // Cowlib calls both qualified; neither is auto-imported.
        assert!(!is_auto_import("hash", 2));
        assert_eq!(lookup_ffi_symbol("crypto", "hash", 2), Some("nera_rt_crypto_hash"));
        assert_eq!(lookup_ffi_symbol("crypto", "strong_rand_bytes", 1),
                   Some("nera_rt_crypto_strong_rand_bytes"));
    }

    #[test]
    fn iolist_size_is_available_as_a_bif() {
        assert!(is_auto_import("iolist_size", 1));
        assert_eq!(lookup_ffi_symbol("erlang", "iolist_size", 1),
                   Some("nera_rt_iolist_size"));
    }

    #[test]
    fn test_lookup_ffi_symbol() {
        assert_eq!(lookup_ffi_symbol("erlang", "length", 1), Some("nera_rt_length"));
        assert_eq!(lookup_ffi_symbol("erlang", "hd", 1), Some("nera_rt_hd"));
        assert_eq!(lookup_ffi_symbol("io", "format", 2), Some("nera_rt_io_format"));
        assert_eq!(lookup_ffi_symbol("maps", "get", 2), Some("nera_rt_map_get"));
        assert_eq!(lookup_ffi_symbol("gen_tcp", "listen", 2), Some("nera_rt_gen_tcp_listen"));
        assert_eq!(lookup_ffi_symbol("file", "read_file", 1), Some("nera_rt_file_read_file"));
        assert_eq!(lookup_ffi_symbol("file", "write_file", 2), Some("nera_rt_file_write_file"));
        // NERA-66: both node arities now have runtime implementations.
        assert_eq!(lookup_ffi_symbol("erlang", "node", 0), Some("nera_rt_node"));
        assert_eq!(lookup_ffi_symbol("erlang", "node", 1), Some("nera_rt_node_1"));
        // Falls through to apply
        assert_eq!(lookup_ffi_symbol("erlang", "apply", 2), None);
        assert_eq!(lookup_ffi_symbol("erlang", "apply", 3), None);
        // Unknown function
        assert_eq!(lookup_ffi_symbol("erlang", "nonexistent", 1), None);
        assert_eq!(lookup_ffi_symbol("unknown_mod", "foo", 0), None);
    }

    #[test]
    fn unsupported_otp_auto_imports_are_not_available_bifs() {
        for (name, arity) in [("open_port", 2), ("port_close", 1), ("is_port", 1),
                              ("spawn_link", 2)] {
            assert!(is_auto_import(name, arity));
            assert!(!is_known_bif("erlang", name, arity));
        }
        assert!(!is_auto_import("open_port", 1));
    }

    #[test]
    fn test_is_auto_import() {
        // Erlang module BIFs are auto-imported
        assert!(is_auto_import("length", 1));
        assert!(is_auto_import("hd", 1));
        assert!(is_auto_import("abs", 1));
        assert!(is_auto_import("is_integer", 1));
        assert!(is_auto_import("spawn", 1));
        assert!(is_auto_import("spawn", 3));
        assert!(is_auto_import("self", 0));
        assert!(is_auto_import("apply", 2));
        assert!(is_auto_import("apply", 3));
        assert!(is_auto_import("throw", 1));
        assert!(is_auto_import("error", 2));

        // Non-erlang module BIFs are NOT auto-imported
        assert!(!is_auto_import("format", 1));    // io:format
        assert!(!is_auto_import("format", 2));    // io:format
        assert!(!is_auto_import("listen", 2));    // gen_tcp:listen
        assert!(!is_auto_import("read_file", 1));  // file:read_file
        assert!(!is_auto_import("write_file", 2)); // file:write_file

        // Wrong arity
        assert!(!is_auto_import("length", 2));
        assert!(!is_auto_import("hd", 0));

        // Unknown
        assert!(!is_auto_import("nonexistent", 1));
    }

    #[test]
    fn test_no_duplicate_entries() {
        for (i, a) in BIF_TABLE.iter().enumerate() {
            for (j, b) in BIF_TABLE.iter().enumerate() {
                if i != j {
                    assert!(
                        !(a.module == b.module && a.function == b.function && a.arity == b.arity),
                        "Duplicate BIF entry: {}:{}/{}",
                        a.module, a.function, a.arity
                    );
                }
            }
        }
    }

    #[test]
    fn test_lookup_bif_param_names() {
        // Multi-arg BIFs should return parameter names
        assert_eq!(lookup_bif_param_names("erlang", "element", 2), Some(["N", "Tuple"].as_slice()));
        assert_eq!(lookup_bif_param_names("erlang", "setelement", 3), Some(["Index", "Tuple", "Value"].as_slice()));
        assert_eq!(lookup_bif_param_names("erlang", "spawn", 3), Some(["Module", "Function", "Args"].as_slice()));
        assert_eq!(lookup_bif_param_names("maps", "get", 2), Some(["Key", "Map"].as_slice()));
        assert_eq!(lookup_bif_param_names("lists", "keyfind", 3), Some(["Key", "N", "TupleList"].as_slice()));
        assert_eq!(lookup_bif_param_names("gen_tcp", "connect", 3), Some(["Address", "Port", "Options"].as_slice()));
        assert_eq!(lookup_bif_param_names("ets", "lookup", 2), Some(["Table", "Key"].as_slice()));
        // Single-arg type checks intentionally return None
        assert_eq!(lookup_bif_param_names("erlang", "is_integer", 1), None);
        assert_eq!(lookup_bif_param_names("erlang", "is_atom", 1), None);
        // Single-arg conversions return None
        assert_eq!(lookup_bif_param_names("erlang", "atom_to_list", 1), None);
        // Zero-arity returns None
        assert_eq!(lookup_bif_param_names("erlang", "self", 0), None);
        // Unknown BIF returns None
        assert_eq!(lookup_bif_param_names("erlang", "nonexistent", 1), None);
    }

    #[test]
    fn test_param_names_arity_matches() {
        for entry in BIF_TABLE {
            if let Some(names) = entry.param_names {
                assert_eq!(
                    names.len(), entry.arity as usize,
                    "param_names length mismatch for {}:{}/{}: got {} names for arity {}",
                    entry.module, entry.function, entry.arity, names.len(), entry.arity
                );
            }
        }
    }

    #[test]
    fn test_lookup_bif_spec() {
        // Erlang module BIFs
        assert_eq!(lookup_bif_spec("erlang", "length", 1), Some("-spec length(List) -> non_neg_integer()."));
        assert_eq!(lookup_bif_spec("erlang", "self", 0), Some("-spec self() -> pid()."));
        assert_eq!(lookup_bif_spec("erlang", "spawn", 3), Some("-spec spawn(Module, Function, Args) -> pid()."));
        assert_eq!(lookup_bif_spec("erlang", "is_integer", 1), Some("-spec is_integer(Term) -> boolean()."));
        // Non-erlang modules
        assert_eq!(lookup_bif_spec("maps", "get", 2), Some("-spec get(Key, Map) -> Value."));
        assert_eq!(lookup_bif_spec("lists", "keyfind", 3), Some("-spec keyfind(Key, N, TupleList) -> Tuple | false."));
        assert_eq!(lookup_bif_spec("ets", "new", 2), Some("-spec new(Name, Options) -> tid() | atom()."));
        // Unknown
        assert_eq!(lookup_bif_spec("erlang", "nonexistent", 1), None);
    }

    #[test]
    fn test_all_bifs_have_spec() {
        for entry in BIF_TABLE {
            assert!(
                entry.spec.is_some(),
                "BIF {}:{}/{} is missing a spec",
                entry.module, entry.function, entry.arity
            );
        }
    }


}
