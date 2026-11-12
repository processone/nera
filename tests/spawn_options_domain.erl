-module(spawn_options_domain).
-export([test/0, noop/0]).
noop() -> ok.
test() ->
    lists:foreach(fun(Opts) ->
        bad(fun() -> spawn_opt(fun noop/0, Opts) end),
        bad(fun() -> spawn_opt(?MODULE, noop, [], Opts) end)
    end, [invalid, [link | invalid], [{fullsweep_after, 1} | invalid],
        [{fullsweep_after, 4294967296}], [{fullsweep_after, -1}],
        [{min_heap_size, -1}], [{max_heap_size, -1}], [unknown]]),
    Old = process_flag(fullsweep_after, 7),
    bad(fun() -> process_flag(fullsweep_after, 4294967296) end),
    7 = process_flag(fullsweep_after, 4294967295),
    4294967295 = process_flag(fullsweep_after, Old),
    Parent = self(),
    {Pid, Ref} = spawn_opt(fun() ->
        4294967295 = process_flag(fullsweep_after, 1), Parent ! valid_spawn
    end, [monitor, {fullsweep_after, 4294967295}]),
    receive valid_spawn -> ok after 2000 -> error(spawn_timeout) end,
    receive {'DOWN', Ref, process, Pid, normal} -> ok after 2000 -> error(down_timeout) end,
    ok.
bad(F) -> {'EXIT', {badarg, _}} = (catch F()), ok.
