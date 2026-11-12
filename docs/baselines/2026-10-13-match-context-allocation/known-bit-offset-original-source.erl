%% EXPECT: ok
-module(reuse_test).
-export([start/0]).
start() -> test(), halt(0).

test() ->
    B = binary:copy(<<1,2,3,4>>, 8192),
    81920 = sum(B, 0),
    %% Interleave reused match contexts with inline tuple/cons allocation,
    %% exhausting regions and crossing GC/yield boundaries while retaining data.
    Pairs = collect(B, 0, []),
    32768 = length(Pairs),
    ok = check(Pairs, 32767),
    81920 = sum(B, 0),
    Parent = self(),
    spawn(fun() -> Parent ! {result, sum(B, 0)} end),
    receive {result, 81920} -> ok after 5000 -> error(timeout) end,
    no = reject(42),
    no = reject({binary, B}),
    3 = bits(<<1:1,0:1,1:1,1:1>>, 0),
    io:format("ok~n").

sum(<<>>, A) -> A;
sum(<<C, Rest/binary>>, A) -> sum(Rest, A+C).
collect(<<>>, _, A) -> A;
collect(<<C, Rest/binary>>, N, A) -> collect(Rest, N+1, [{N,C}|A]).
check([], -1) -> ok;
check([{N,C}|Rest], N) ->
    C = 1 + N rem 4,
    check(Rest, N-1).
reject(<<_:8, _/binary>>) -> yes;
reject(_) -> no.
bits(<<>>, A) -> A;
bits(<<C:1, Rest/bitstring>>, A) -> bits(Rest, A+C).
