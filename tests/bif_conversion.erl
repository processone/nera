%% Adapted from OTP stdlib test suites (Apache 2.0)
%% Tests type conversion BIFs.
-module(bif_conversion).
-export([test/0]).

test() ->
    %% atom_to_list
    "hello" = atom_to_list(hello),
    "true" = atom_to_list(true),
    "ok" = atom_to_list(ok),

    %% list_to_atom
    hello = list_to_atom("hello"),
    true = list_to_atom("true"),

    %% integer_to_list
    "42" = integer_to_list(42),
    "0" = integer_to_list(0),
    "-1" = integer_to_list(-1),
    "1000" = integer_to_list(1000),

    %% list_to_integer
    42 = list_to_integer("42"),
    0 = list_to_integer("0"),
    -1 = list_to_integer("-1"),

    %% Roundtrip: integer -> list -> integer
    12345 = list_to_integer(integer_to_list(12345)),
    -999 = list_to_integer(integer_to_list(-999)),

    %% Roundtrip: atom -> list -> atom
    hello = list_to_atom(atom_to_list(hello)),

    %% tuple_to_list
    [1, 2, 3] = tuple_to_list({1, 2, 3}),
    [] = tuple_to_list({}),
    [a] = tuple_to_list({a}),

    %% list_to_tuple
    {1, 2, 3} = list_to_tuple([1, 2, 3]),
    {} = list_to_tuple([]),
    {a} = list_to_tuple([a]),

    %% Roundtrip: tuple -> list -> tuple
    {a, b, c} = list_to_tuple(tuple_to_list({a, b, c})),

    %% float_to_list (basic)
    true = is_list(float_to_list(3.14)),

    io:format("ok~n"),
    ok.
