%% Tests for binary conversion BIFs added for onecached support.
-module(bif_binary_conv).
-export([test/0]).

test() ->
    %% integer_to_binary
    <<"42">> = integer_to_binary(42),
    <<"0">> = integer_to_binary(0),
    <<"-1">> = integer_to_binary(-1),
    <<"1000000">> = integer_to_binary(1000000),

    %% binary_to_integer
    42 = binary_to_integer(<<"42">>),
    0 = binary_to_integer(<<"0">>),
    -1 = binary_to_integer(<<"-1">>),
    999 = binary_to_integer(<<"999">>),

    %% Roundtrip integer -> binary -> integer
    12345 = binary_to_integer(integer_to_binary(12345)),
    -999 = binary_to_integer(integer_to_binary(-999)),

    %% list_to_binary
    <<"hello">> = list_to_binary([104, 101, 108, 108, 111]),
    <<>> = list_to_binary([]),
    <<"A">> = list_to_binary([65]),

    %% binary_to_list
    [104, 101, 108, 108, 111] = binary_to_list(<<"hello">>),
    [] = binary_to_list(<<>>),
    [65] = binary_to_list(<<"A">>),

    %% Roundtrip binary -> list -> binary
    <<"test">> = list_to_binary(binary_to_list(<<"test">>)),

    %% iolist_to_binary
    <<"hello">> = iolist_to_binary([<<"hello">>]),
    <<"helloworld">> = iolist_to_binary([<<"hello">>, <<"world">>]),
    <<"abc">> = iolist_to_binary([97, 98, 99]),
    <<"mixed">> = iolist_to_binary([<<"mix">>, [101, 100]]),
    <<>> = iolist_to_binary([]),
    %% Nested iolist
    <<"abcd">> = iolist_to_binary([[97, 98], [99, 100]]),

    %% atom_to_binary
    <<"hello">> = atom_to_binary(hello),
    <<"true">> = atom_to_binary(true),
    <<"ok">> = atom_to_binary(ok),

    %% binary_to_atom
    hello = binary_to_atom(<<"hello">>),
    true = binary_to_atom(<<"true">>),
    ok = binary_to_atom(<<"ok">>),

    %% Roundtrip atom -> binary -> atom
    world = binary_to_atom(atom_to_binary(world)),

    io:format("ok~n"),
    ok.
