%% EXPECT: deep_term_ops_ok
%% Regression: <, >, term_to_binary/1, iolist_to_binary/1 and phash2/1 on
%% 200,000-level head-nested iolists must finish with BEAM's results.
%% Run this same fixture on native Nera and BEAM. ETF bytes and flattened
%% payloads are checked in full, without decoding a deeply nested ETF term.
-module(deep_term_ops).
-export([test/0, hash_readings/0]).

-define(DEPTH, 200000).

test() ->
    L = build(?DEPTH, []),
    Other = build(?DEPTH, [0]),
    true = L < Other,
    true = Other > L,
    false = L > Other,
    false = Other < L,
    Equal = build(?DEPTH, []),
    false = L < Equal,
    false = L > Equal,
    %% Equal heads must resume the saved tail comparison, not return early.
    true = [L, <<"a">>] < [Equal, <<"b">>],
    true = [L, <<"b">>] > [Equal, <<"a">>],

    Payload = binary:copy(<<"chunk">>, ?DEPTH),
    Payload = iolist_to_binary(L),
    %% Each level is LIST_EXT(2), then the inner term, then BINARY_EXT and NIL.
    Prefix = binary:copy(<<108, 0, 0, 0, 2>>, ?DEPTH),
    Suffix = binary:copy(<<109, 0, 0, 0, 5, "chunk", 106>>, ?DEPTH),
    Encoded = <<131, Prefix/binary, 106, Suffix/binary>>,
    Encoded = term_to_binary(L),
    %% OTP 29.1.2 reading, reproducible with hash_readings/0 below.
    108113888 = erlang:phash2(L),

    %% Portable hashes must match OTP, not only be in range. These inputs
    %% cover the leaf and container paths used inside nested term graphs.
    lists:foreach(fun({Term, Hash}) -> Hash = erlang:phash2(Term) end, hash_cases()),
    io:format("deep_term_ops_ok~n"),
    ok.

build(0, Acc) -> Acc;
build(N, Acc) -> build(N - 1, [Acc, <<"chunk">>]).

%% Read from BEAM, never from a reimplementation of its hash algorithm:
%% erlc -o /tmp tests/deep_term_ops.erl
%% erl -noshell -pa /tmp -eval 'deep_term_ops:hash_readings(), halt().'
hash_readings() ->
    lists:foreach(fun({Term, _}) -> io:format("~p~n", [erlang:phash2(Term)]) end,
                  hash_cases()),
    io:format("deep: ~p~n", [erlang:phash2(build(?DEPTH, []))]).

hash_cases() ->
    [{[], 113427502}, {0, 88723725}, {1, 2614250}, {-1, 44071773},
     {134217727, 112602999}, {134217728, 12354923},
     {-134217728, 69672967}, {-134217729, 76739502},
     {1 bsl 62, 29238129}, {-(1 bsl 62), 82210594}, {1 bsl 100, 126219984},
     {0.0, 20875736}, {-0.0, 20875736}, {1.0, 77147068}, {-1.5, 13753666},
     {hello, 7258927}, {'é', 233}, {'λ', 3483},
     {<<>>, 13708901}, {<<"chunk">>, 33132665}, {<<1:3>>, 73037028},
     {<< <<I>> || I <- lists:seq(0, 255) >>, 3064723},
     {[1, 2, 3, 4, 5], 66056394}, {[1, 2 | <<"chunk">>], 82515919},
     {[0, hello, 255], 60092478}, {{}, 87486268},
     {{hello, [1, 2], <<"chunk">>}, 56268936},
     {#{}, 39679005}, {#{a => 1, b => 2}, 103634663},
     {#{a => #{x => 1}, b => [2, 3]}, 69340935},
     {build(2, []), 113444212}].
