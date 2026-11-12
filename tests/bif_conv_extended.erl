%% Tests for extended conversion BIFs needed by thoas and similar libraries.
-module(bif_conv_extended).
-export([test/0]).

test() ->
    %% ── integer_to_binary/2 — base conversion ──
    <<"FF">> = integer_to_binary(255, 16),
    <<"1010">> = integer_to_binary(10, 2),
    <<"77">> = integer_to_binary(63, 8),
    <<"0">> = integer_to_binary(0, 16),
    <<"-A">> = integer_to_binary(-10, 16),
    <<"Z">> = integer_to_binary(35, 36),
    <<"10">> = integer_to_binary(2, 2),

    %% ── integer_to_list/2 — base conversion to list ──
    "FF" = integer_to_list(255, 16),
    "1010" = integer_to_list(10, 2),
    "77" = integer_to_list(63, 8),
    "0" = integer_to_list(0, 16),
    "-A" = integer_to_list(-10, 16),

    %% ── binary_to_float/1 ──
    %% Standard float parsing
    3.14 = binary_to_float(<<"3.14">>),
    1.0 = binary_to_float(<<"1.0">>),
    -2.5 = binary_to_float(<<"-2.5">>),
    0.0 = binary_to_float(<<"0.0">>),

    %% ── binary_to_existing_atom/1 ──
    %% These atoms already exist because they are used above
    ok = binary_to_existing_atom(<<"ok">>),
    true = binary_to_existing_atom(<<"true">>),
    error = binary_to_existing_atom(<<"error">>),

    %% ── binary_to_existing_atom/1 — unknown atom raises badarg ──
    ok = try binary_to_existing_atom(<<"no_such_atom_xyzzy_42">>)
    catch error:badarg -> ok
    end,

    %% ── atom_to_binary/2 — with encoding ──
    <<"hello">> = atom_to_binary(hello, utf8),
    <<"ok">> = atom_to_binary(ok, utf8),

    %% ── float_to_binary/1 — default formatting ──
    %% Just check it returns a binary (exact format is implementation-defined)
    true = is_binary(float_to_binary(3.14)),

    %% ── float_to_binary/2 — with [short] option ──
    Bin = float_to_binary(3.14, [short]),
    true = is_binary(Bin),
    %% The short representation should be concise
    true = byte_size(Bin) < 20,

    io:format("ok~n"),
    ok.
