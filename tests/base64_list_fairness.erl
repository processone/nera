%% EXPECT: ok
%% Logical progress, not a wall-clock bound: GC remains synchronous.
-module(base64_list_fairness).
-export([test/0, single_scheduler/0]).
test() -> fairness(false), io:format("ok~n").
single_scheduler() -> fairness(true), io:format("ok~n").
outcome(Input) -> try base64:decode(Input, #{}) of V -> {ok,V} catch C:R -> {C,R} end.
fairness(Strict) ->
    Space = lists:duplicate(256*1024,$\s),
    Data = lists:duplicate(256*1024,$A),
    lists:foreach(fun({Input, Expected}) -> compete(Input, Expected, Strict) end,
        [{Space,{ok,<<>>}}, {Space++"Zg==",{ok,<<"f">>}},
         {Space++"Y===",{error,badarith}},
         {"Zg=="++Space++"*",{error,{case_clause,bad}}},
         {Data,{ok,binary:copy(<<0>>,192*1024)}}]).
compete(Input, Expected, Strict) ->
    Parent = self(),
    Observer = spawn(fun() -> Parent ! ready, receive go -> Parent ! observed end end),
    receive ready -> ok after 10000 -> error(observer_not_ready) end,
    Worker = spawn(fun() -> receive go ->
        Observer ! go,
        Expected = outcome(Input),
        Parent ! decoded
    end end),
    Worker ! go,
    receive
        observed -> receive decoded -> ok after 10000 -> error(decoder_timeout) end;
        decoded when Strict =:= false -> receive observed -> ok after 10000 -> error(observer_timeout) end;
        decoded -> error(observer_starved)
    after 10000 -> error(no_progress)
    end.
