-module(spawn_probe).
-export([main/0, child/1]).
main() ->
    bench(closure_empty, closure, []),
    bench(closure_heap, closure, [{fullsweep_after, 10}, {min_heap_size, 256}, {max_heap_size, 100000}]),
    bench(mfa_heap, mfa, [{fullsweep_after, 10}, {min_heap_size, 256}, {max_heap_size, 100000}]).
bench(Name, Kind, Opts) ->
    loop(Kind, Opts, 16),
    {Time, ok} = timer:tc(fun() -> loop(Kind, Opts, 2000) end),
    io:format("~p ~p~n", [Name, Time]).
loop(_, _, 0) -> ok;
loop(Kind, Opts, N) ->
    Parent = self(),
    Pid = case Kind of
        closure -> spawn_opt(fun() -> Parent ! {done, self()} end, Opts);
        mfa -> spawn_opt(?MODULE, child, [Parent], Opts)
    end,
    receive {done, Pid} -> ok after 5000 -> error(spawn_timeout) end,
    loop(Kind, Opts, N - 1).
child(Parent) -> Parent ! {done, self()}.
