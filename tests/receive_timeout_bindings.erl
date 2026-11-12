%% EXPECT: receive_timeout_bindings_ok
%% Regression: a name bound in a receive's after timeout expression stays
%% bound after the receive, as erlc allows and BEAM runs: in
%% `receive never -> ok after (T = id(0)) -> ok end, T` the last read
%% returns 0. The timeout runs before any message is taken, so the name is
%% bound whether a message clause or the after body ends the receive. Before
%% the fix, the read failed to compile with "variable 'T' is unbound". The
%% same fixture runs on BEAM.
-module(receive_timeout_bindings).
-export([test/0]).

test() ->
    0 = timeout_read(),
    {late, 5} = timeout_and_result(),
    {got, 100} = message_path_read(),
    {done, 0} = block_timeout_read(),
    {0, <<7, 8>>} = binary_tail_read(id(<<0, 7, 8>>)),
    {matched, zero, 0, key} = later_uses(),
    io:format("receive_timeout_bindings_ok~n"),
    ok.

%% The example from the report: only the timeout binds T.
timeout_read() ->
    receive never -> ok after (T = id(0)) -> ok end,
    T.

%% The receive's own value and the timeout binding are both read.
timeout_and_result() ->
    X = receive never -> ok after (T = id(5)) -> late end,
    {X, T}.

%% A waiting message ends the receive, and the timeout still bound T.
message_path_read() ->
    self() ! go,
    R = receive go -> got after (T = id(100)) -> late end,
    {R, T}.

%% A block in the timeout binds a name before it returns the timeout.
block_timeout_read() ->
    R = receive never -> ok after begin W = id(0), W end -> done end,
    {R, W}.

%% A binary tail matched in the timeout is read after the receive.
binary_tail_read(Bin) ->
    receive never -> ok after begin <<N, Rest/binary>> = Bin, N end -> ok end,
    {N, Rest}.

%% Later patterns, guards, fun captures and fun-head map keys read the
%% timeout binding.
later_uses() ->
    receive never -> ok after (T = id(0)) -> ok end,
    {T, Tag} = {0, matched},
    Name = case id(0) of
        V when V =:= T -> zero;
        _ -> other
    end,
    F = fun() -> T end,
    G = fun(#{T := K}) -> K end,
    {Tag, Name, F(), G(#{0 => key, 1 => other})}.

id(X) -> X.
