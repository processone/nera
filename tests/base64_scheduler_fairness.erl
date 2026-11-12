%% EXPECT: ok
-module(base64_scheduler_fairness).
-export([test/0, single_scheduler/0]).

%% General suite: check results and completion without making assumptions about
%% OS scheduling between different workers. The dedicated runner selects one
%% scheduler and additionally requires the observer to beat the encoder.
test() -> run(false).
single_scheduler() -> run(true).
run(Strict) ->
    lists:foreach(fun({Size, Count}) ->
        compete(binary:copy(<<0>>, Size), Count, Strict)
    end, [{49149,128}, {49152,128}, {3*1024*1024,1}]),
    io:format("ok~n").

compete(Input, Count, Strict) ->
    Parent = self(),
    Observer = spawn(fun() ->
        Parent ! observer_ready,
        receive go -> Parent ! observed end
    end),
    receive observer_ready -> ok after 10000 -> error(observer_not_ready) end,
    Worker = spawn(fun() ->
        receive go ->
            Observer ! go,
            encode_loop(Input, Count, ((byte_size(Input)+2) div 3)*4),
            Parent ! encoded
        end
    end),
    Worker ! go,
    receive
        observed -> receive encoded -> ok after 10000 -> error(encoder_timeout) end;
        encoded when Strict =:= false ->
            receive observed -> ok after 10000 -> error(observer_timeout) end;
        encoded -> error(observer_starved)
    after 10000 -> error(no_progress)
    end.

encode_loop(_, 0, _) -> ok;
encode_loop(Input, N, ExpectedSize) ->
    Result = base64:encode(Input, #{}),
    ExpectedSize = byte_size(Result),
    $A = binary:at(Result, 0),
    encode_loop(Input, N-1, ExpectedSize).
