%% EXPECT: binary_copy_scheduler_ok
%% Intent: binary:copy/2 must account for output work and yield while copying.
%% The dedicated single-scheduler entry asserts ordering, not elapsed time.
-module(binary_copy_scheduler).
-export([test/0, single_scheduler/0]).

test() -> run(false).
single_scheduler() -> run(true).

run(Strict) ->
    %% One large call and repeated near-chunk calls must both share a worker.
    lists:foreach(fun({Size, Count}) -> compete(Size, Count, Strict) end,
                  [{65535,128}, {65536,128}, {65537,128}, {4*1024*1024,1}]),
    io:format("binary_copy_scheduler_ok~n"),
    ok.

compete(Size, Count, Strict) ->
    Parent = self(),
    Observer = spawn(fun() ->
        Parent ! observer_ready,
        receive go -> Parent ! observed end
    end),
    receive observer_ready -> ok after 5000 -> error(observer_not_ready) end,
    Worker = spawn(fun() ->
        receive go ->
            Observer ! go,
            copy_loop(Size, Count),
            Parent ! copied
        end
    end),
    Worker ! go,
    receive
        observed -> receive copied -> ok after 5000 -> error(copy_timeout) end;
        copied when Strict =:= false ->
            receive observed -> ok after 5000 -> error(observer_timeout) end;
        copied -> error(observer_starved)
    after 5000 -> error(no_copy_progress)
    end.

copy_loop(_, 0) -> ok;
copy_loop(Size, Count) ->
    Result = binary:copy(<<42>>, Size),
    Size = byte_size(Result),
    42 = binary:at(Result, Size-1),
    copy_loop(Size, Count-1).
