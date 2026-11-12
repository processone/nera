-module(gc_idle_live_roots).
-export([test/0]).
%% EXPECT: gc_idle_live_roots ok

test() ->
    Parent = self(),
    Worker = spawn(fun() ->
        Input = lists:duplicate(300000, $A),
        idle(Input, 8),
        Result = base64:decode(Input, #{}),
        true = Result =:= binary:copy(<<0>>, 225000),
        Parent ! complete
    end),
    receive complete -> ok after 10000 -> error({timeout, Worker}) end,
    io:format("gc_idle_live_roots ok~n").

idle(Input, 0) -> 300000 = length(Input), ok;
idle(Input, N) ->
    receive after 1 -> ok end,
    $A = hd(Input),
    idle(Input, N - 1).
