%% Regression test for is_tuple/is_float guard codegen.
%%
%% The is_tuple type test previously only checked TAG_BOXED (0b000) which
%% is shared by ALL boxed types (maps, binaries, floats, closures, etc.).
%% Maps, binaries, and floats would incorrectly pass is_tuple guards.
%% Same issue for is_float (matched any boxed type).
-module(is_tuple_guard).
-export([test/0]).

test() ->
    %% ── is_tuple guard must distinguish tuple from other boxed types ──
    true = is_tuple({a, b}),
    true = is_tuple({1, 2, 3}),
    true = is_tuple({}),
    false = is_tuple(#{a => 1}),
    false = is_tuple(<<"binary">>),
    false = is_tuple(3.14),
    false = is_tuple(42),
    false = is_tuple(hello),
    false = is_tuple([1, 2]),
    false = is_tuple(fun() -> ok end),
    false = is_tuple(make_ref()),

    %% ── is_float guard must distinguish float from other boxed types ──
    true = is_float(3.14),
    true = is_float(0.0),
    true = is_float(-1.5),
    false = is_float(42),
    false = is_float({1, 2}),
    false = is_float(#{a => 1}),
    false = is_float(<<"binary">>),
    false = is_float(hello),
    false = is_float(fun() -> ok end),
    false = is_float(make_ref()),

    %% ── Guards in function clauses ──
    tuple = classify({a, b}),
    map = classify(#{a => 1}),
    float = classify(3.14),
    binary = classify(<<"hello">>),
    list = classify([1, 2]),
    integer = classify(42),
    atom = classify(hello),
    closure = classify(fun() -> ok end),
    reference = classify(make_ref()),

    %% ── Tuple pattern must not match maps (the thoas bug) ──
    other = tuple_or_other(#{x => 1}),
    other = tuple_or_other(<<"bin">>),
    other = tuple_or_other(3.14),
    tuple = tuple_or_other({a, b}),

    %% ── Keyword list pattern [{_, _}|_] must not match [Map|_] ──
    kwlist = kw_or_list([{a, 1}]),
    list = kw_or_list([#{a => 1}]),
    list = kw_or_list([1, 2, 3]),
    other = kw_or_list(42),

    %% ── is_tuple in case expression guard ──
    yes = case #{a => 1} of V1 when is_tuple(V1) -> no; _ -> yes end,
    yes = case {1, 2} of V2 when is_tuple(V2) -> yes; _ -> no end,

    %% ── is_float in case expression guard ──
    yes = case #{a => 1} of V3 when is_float(V3) -> no; _ -> yes end,
    yes = case 3.14 of V4 when is_float(V4) -> yes; _ -> no end,

    %% ── is_tuple in if guard ──
    T = {a, b},
    NT = #{a => 1},
    yes = if is_tuple(T) -> yes; true -> no end,
    no = if is_tuple(NT) -> yes; true -> no end,

    %% ── tuple_size on non-tuples must fail gracefully ──
    bad = safe_tuple_size(#{a => 1}),
    bad = safe_tuple_size(<<"bin">>),
    bad = safe_tuple_size(3.14),
    2 = safe_tuple_size({a, b}),
    0 = safe_tuple_size({}),

    %% ── Combined guards: is_tuple andalso tuple_size ──
    yes = sized_tuple({a, b, c}, 3),
    no = sized_tuple({a, b}, 3),
    no = sized_tuple(#{a => 1}, 3),
    no = sized_tuple(42, 3),

    %% ── Clause ordering: exact thoas value/2 pattern ──
    %% The value function has guards for atom, binary, integer, float,
    %% a [{_,_}|_] pattern, then list and map guards. Regression for
    %% the bug where maps matched the tuple pattern.
    atom_val = thoas_value(null),
    binary_val = thoas_value(<<"hello">>),
    integer_val = thoas_value(42),
    float_val = thoas_value(3.14),
    kwlist_val = thoas_value([{<<"key">>, <<"val">>}]),
    list_val = thoas_value([1, 2, 3]),
    map_val = thoas_value(#{<<"a">> => 1}),

    io:format("ok~n"),
    ok.

classify(V) when is_tuple(V) -> tuple;
classify(V) when is_map(V) -> map;
classify(V) when is_float(V) -> float;
classify(V) when is_binary(V) -> binary;
classify(V) when is_list(V) -> list;
classify(V) when is_integer(V) -> integer;
classify(V) when is_function(V) -> closure;
classify(V) when is_reference(V) -> reference;
classify(V) when is_atom(V) -> atom;
classify(_) -> unknown.

tuple_or_other({_, _}) -> tuple;
tuple_or_other(_) -> other.

kw_or_list([{_, _} | _]) -> kwlist;
kw_or_list(V) when is_list(V) -> list;
kw_or_list(_) -> other.

safe_tuple_size(V) when is_tuple(V) -> tuple_size(V);
safe_tuple_size(_) -> bad.

sized_tuple(V, N) when is_tuple(V), tuple_size(V) == N -> yes;
sized_tuple(_, _) -> no.

%% Mimics the exact clause order of thoas_encode:value/2
thoas_value(V) when is_atom(V) -> atom_val;
thoas_value(V) when is_binary(V) -> binary_val;
thoas_value(V) when is_integer(V) -> integer_val;
thoas_value(V) when is_float(V) -> float_val;
thoas_value([{_, _} | _]) -> kwlist_val;
thoas_value(V) when is_list(V) -> list_val;
thoas_value(V) when is_map(V) -> map_val;
thoas_value(_) -> unknown_val.
