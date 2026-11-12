%% Adapted from OTP compiler/test/guard_SUITE.erl (Apache 2.0)
%% Tests type-testing BIFs.
-module(bif_type_tests).
-export([test/0]).

test() ->
    %% is_integer
    true = is_integer(42),
    true = is_integer(-1),
    true = is_integer(0),
    false = is_integer(3.14),
    false = is_integer(hello),
    false = is_integer([]),

    %% is_float
    true = is_float(3.14),
    true = is_float(0.0),
    false = is_float(42),
    false = is_float(hello),

    %% is_number
    true = is_number(42),
    true = is_number(3.14),
    false = is_number(hello),
    false = is_number([]),

    %% is_atom
    true = is_atom(hello),
    true = is_atom(true),
    true = is_atom(false),
    false = is_atom(42),
    false = is_atom([]),

    %% is_boolean
    true = is_boolean(true),
    true = is_boolean(false),
    false = is_boolean(hello),
    false = is_boolean(1),
    false = is_boolean(0),

    %% is_list
    true = is_list([]),
    true = is_list([1, 2]),
    true = is_list("hello"),
    false = is_list(42),
    false = is_list({}),

    %% is_tuple
    true = is_tuple({}),
    true = is_tuple({1, 2}),
    false = is_tuple([]),
    false = is_tuple(42),

    %% is_binary
    true = is_binary(<<>>),
    true = is_binary(<<1, 2, 3>>),
    false = is_binary(42),
    false = is_binary([]),

    %% is_map
    true = is_map(#{}),
    true = is_map(#{a => 1}),
    false = is_map(42),
    false = is_map([]),

    %% is_pid
    true = is_pid(self()),
    false = is_pid(42),
    false = is_pid(hello),

    io:format("ok~n"),
    ok.
