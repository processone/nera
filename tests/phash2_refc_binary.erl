%% Tests that erlang:phash2 produces identical hashes for the same binary
%% content whether stored as a heap binary or a refc binary (sub-binary
%% of a large off-heap binary).
-module(phash2_refc_binary).
-export([test/0]).

test() ->
    %% Build a binary large enough to be a refc binary (>64 bytes)
    Big = list_to_binary(lists:seq(0, 99)),
    %% Extract a sub-binary (shares the refc backing store)
    <<Sub:10/binary, _/binary>> = Big,
    %% Same content as a small heap binary
    Small = list_to_binary(lists:seq(0, 9)),
    %% Hashes must match — both contain bytes 0..9
    H1 = erlang:phash2(Sub),
    H2 = erlang:phash2(Small),
    H1 = H2,
    ok.
