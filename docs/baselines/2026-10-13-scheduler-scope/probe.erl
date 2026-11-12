-module(probe).
-export([test/0]).

test() ->
    Self = self(),
    true = register(scope_probe, Self),
    {LookupTime, ok} = timer:tc(fun() -> lookups(10000000, Self) end),
    Target = spawn(fun() -> receive stop -> ok end end),
    {MonitorTime, ok} = timer:tc(fun() -> monitors(3000000, Target) end),
    {SendTime, ok} = timer:tc(fun() -> self_sends(10000000) end),
    true = unregister(scope_probe),
    Target ! stop,
    io:format("lookup ~p~nmonitor ~p~nself_send ~p~n", [LookupTime, MonitorTime, SendTime]),
    ok.

lookups(0, _) -> ok;
lookups(N, Self) ->
    Self = whereis(scope_probe),
    true = is_process_alive(Self),
    lookups(N - 1, Self).

monitors(0, _) -> ok;
monitors(N, Target) ->
    Ref = monitor(process, Target),
    true = demonitor(Ref),
    monitors(N - 1, Target).

self_sends(0) -> ok;
self_sends(N) ->
    scope_probe ! N,
    receive N -> ok end,
    self_sends(N - 1).
