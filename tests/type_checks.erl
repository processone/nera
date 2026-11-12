-module(type_checks).
-export([test/0]).

id(X) -> X.

test() ->
    %% is_binary
    true = is_binary(id(<<"hello">>)),
    true = is_binary(id(<<1,2,3>>)),
    false = is_binary(id(42)),
    false = is_binary(id(foo)),
    false = is_binary(id([1,2])),
    false = is_binary(id({a,b})),

    %% is_map
    true = is_map(id(#{})),
    true = is_map(id(#{a => 1, b => 2})),
    false = is_map(id(42)),
    false = is_map(id(foo)),
    false = is_map(id([1,2])),
    false = is_map(id({a,b})),

    %% is_function
    F = id(fun(X) -> X end),
    true = is_function(F),
    false = is_function(id(42)),
    false = is_function(id(foo)),
    false = is_function(id([])),

    %% is_reference
    Ref = id(make_ref()),
    true = is_reference(Ref),
    false = is_reference(id(42)),
    false = is_reference(id(foo)),
    false = is_reference(id(self())),

    %% is_number
    true = is_number(id(42)),
    true = is_number(id(-100)),
    true = is_number(id(3.14)),
    false = is_number(id(foo)),
    false = is_number(id(<<"bin">>)),
    false = is_number(id([1])),

    %% is_integer (already inlined, but verify)
    true = is_integer(id(0)),
    true = is_integer(id(-1)),
    false = is_integer(id(3.14)),
    false = is_integer(id(foo)),

    %% is_atom (already inlined, verify)
    true = is_atom(id(foo)),
    true = is_atom(id(true)),
    false = is_atom(id(42)),

    %% is_list (already inlined, verify)
    true = is_list(id([1,2,3])),
    true = is_list(id([])),
    false = is_list(id(42)),

    %% is_pid (already inlined, verify)
    true = is_pid(id(self())),
    false = is_pid(id(42)),

    %% is_tuple (already inlined, verify)
    true = is_tuple(id({a,b})),
    false = is_tuple(id(42)),

    %% is_boolean
    true = is_boolean(id(true)),
    true = is_boolean(id(false)),
    false = is_boolean(id(42)),
    false = is_boolean(id(foo)),
    false = is_boolean(id(ok)),
    false = is_boolean(id([])),
    false = is_boolean(id(<<"true">>)),

    %% Guards with inlined type checks
    ok = guard_binary(<<"hello">>),
    ok = guard_map(#{a => 1}),
    ok = guard_function(fun() -> ok end),
    ok = guard_number(42),
    ok = guard_number(3.14),
    ok = guard_boolean(true),
    ok = guard_boolean(false),
    error = guard_boolean(42),
    error = guard_boolean(ok),

    ok.

guard_binary(X) when is_binary(X) -> ok;
guard_binary(_) -> error.

guard_map(X) when is_map(X) -> ok;
guard_map(_) -> error.

guard_function(X) when is_function(X) -> ok;
guard_function(_) -> error.

guard_number(X) when is_number(X) -> ok;
guard_number(_) -> error.

guard_boolean(X) when is_boolean(X) -> ok;
guard_boolean(_) -> error.
