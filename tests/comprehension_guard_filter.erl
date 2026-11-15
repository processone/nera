%% EXPECT: comprehension_guard_filter_ok
%% Guard-expression filters treat exceptions as false; ordinary filter
%% expressions still raise. Run this same fixture on native Nera and BEAM.
-module(comprehension_guard_filter).
-export([test/0]).
-compile({no_auto_import, [length/1, size/1, tuple_size/1]}).
-import(maps, [size/1]).
-import(erlang, [tuple_size/1]).
-record(options, {enabled = true}).
-record(guard_default, {enabled = erlang:map_size(not_a_map) > 0}).
-record(ordinary_default, {enabled = default_enabled()}).
-record(update_default, {enabled = (#options{})#options{enabled = erlang:map_size(not_a_map) > 0}}).

test() ->
    A = #{a => 1},
    B = #{b => 2},
    Empty = #{},
    Maps = id([A, not_a_map, Empty, B]),
    [A, B] = [X || X <- Maps, map_size(X) > 0],
    [A, B] = [X || X <- Maps, erlang:map_size(X) > 0],
    <<1, 1>> = << <<(map_size(X))>> || X <- Maps, map_size(X) > 0 >>,
    %% Guard failure stops the remaining filters, body and inner source.
    put(trace, []),
    [1, 1] = [event(body, map_size(X)) || X <- Maps, map_size(X) > 0,
        event(filter, true)],
    [filter, body, filter, body] = lists:reverse(erase(trace)),
    put(trace, []),
    [{1, 7}, {1, 7}] = [{map_size(X), Y} || X <- Maps, map_size(X) > 0,
        Y <- event(source, [7])],
    [source, source] = lists:reverse(erase(trace)),
    %% Each generator's filters use guard semantics, including <= generators.
    [1, 2] = [X || <<X>> <= id(<<0, 1, 2>>), 1 / X > 0],
    <<1, 2>> = << <<X>> || <<X>> <= id(<<0, 1, 2>>), 1 / X > 0 >>,
    [{1, 2}] = [{X, Y} || X <- id([0, 1]), 1 / X > 0,
        Y <- id([0, 2]), 1 / Y > 0],
    %% Failing BIFs, inline operations and compound guards all skip.
    [2] = [X || X <- id([0, 2, bad]), 4 div X > 0],
    [A] = [X || X <- Maps, map_get(a, X) > 0],
    [A, B] = [X || X <- Maps, not (map_size(X) =:= 0)],
    [A, Empty, B] = [X || X <- Maps, map_size(X) > 0 orelse true],
    [A, Empty, B] = [X || X <- Maps, is_map(X) andalso map_size(X) >= 0],
    [A, B] = [X || X <- Maps, {map_size(X)} > {0}],
    [A] = [X || X <- Maps, (X#{a := 2}) =:= #{a => 2}],
    [A, Empty, B] = [X || X <- Maps, <<(map_size(X))>> =/= <<255>>],
    [#options{}] = [X || X <- id([#options{}, not_a_record]), X#options.enabled],
    %% Leading and filter-only guards suppress exceptions before any source/body.
    BadMap = id(not_a_map),
    [] = [X || map_size(BadMap) > 0, X <- error(source_ran)],
    <<>> = << <<X>> || map_size(BadMap) > 0, <<X>> <= error(source_ran) >>,
    [] = [error(body_ran) || map_size(BadMap) > 0],
    <<>> = << (error(body_ran)) || map_size(BadMap) > 0 >>,
    [] = try [ok || map_size(BadMap) > 0] catch _:_ -> handler_ran end,
    [] = [ok || #options{enabled = map_size(BadMap) > 0} =:= {options, true}],
    [] = [ok || #guard_default{} =:= {guard_default, true}],
    [] = [ok || #ordinary_default{enabled = true} =:= {ordinary_default, true},
        map_size(BadMap) > 0],
    [ok] = [ok || map_size(id_map()) > 0],
    %% Deferred tails and captures remain usable after a failing guard.
    <<0, Tail/bits>> = id(<<0, 5:3>>),
    [[Tail], []] = [[Tail || map_size(X) > 0] || X <- id([#{a => 1}, bad])],
    3 = bit_size(Tail),
    ordinary_errors(Maps, BadMap),
    record_update_errors(BadMap),
    io:format("comprehension_guard_filter_ok~n"),
    ok.

ordinary_errors(Maps, BadMap) ->
    %% The whole expression must be guard-safe, not just one nested BIF.
    {error, {badmap, not_a_map}} = result(fun() ->
        [X || X <- Maps, id(map_size(X)) > 0]
    end),
    {error, {badmap, not_a_map}} = result(fun() ->
        << <<1>> || X <- Maps, maps:size(X) > 0 >>
    end),
    {error, {badmap, not_a_map}} = result(fun() ->
        [ok || begin map_size(BadMap) > 0 end]
    end),
    {error, default_failed} = result(fun() ->
        [ok || #ordinary_default{} =:= {ordinary_default, true}]
    end),
    %% A local override and a non-erlang import are ordinary calls, while an
    %% erlang import and an explicitly qualified guard BIF remain guard-safe.
    {error, badarg} = result(fun() -> [X || X <- id([[1], bad]), length(X) > 0] end),
    {error, {badmap, not_a_map}} = result(fun() -> [X || X <- Maps, size(X) > 0] end),
    [[1]] = [X || X <- id([[1], bad]), erlang:length(X) > 0],
    [{ok}] = [X || X <- id([{ok}, bad]), tuple_size(X) > 0],
    F = fun erlang:map_size/1,
    {error, {badmap, not_a_map}} = result(fun() -> [X || X <- Maps, F(X) > 0] end),
    %% Exceptions in a generator source or body are not filter failures.
    {error, {badmap, not_a_map}} = result(fun() -> [map_size(X) || X <- Maps] end),
    {error, {badmap, not_a_map}} = result(fun() -> [X || X <- map_size(BadMap)] end),
    {throw, filter_failed} = result(fun() -> [ok || throw(filter_failed)] end),
    {exit, filter_failed} = result(fun() -> [X || X <- Maps, exit(filter_failed)] end),
    %% A skipped guard clears only its exception; a later ordinary filter's
    %% error still reaches the enclosing handler, and nothing after it runs.
    erase(after_filter),
    filter_failed = try
        [X || X <- Maps, map_size(X) > 0, error(filter_failed)],
        put(after_filter, ran),
        missed_handler
    catch error:Reason -> Reason end,
    undefined = get(after_filter),
    ok.

%% OTP classifies record updates as ordinary expressions before expanding
%% records. Their errors propagate in every filter position, including an
%% update nested inside a guard-safe call, tuple or record default.
record_update_errors(BadMap) ->
    R = #options{},
    Cases = [
        fun() -> [ok || (R#options{enabled = map_size(BadMap) > 0}) =:= R] end,
        fun() -> [X || (R#options{enabled = map_size(BadMap) > 0}) =:= R, X <- [ok]] end,
        fun() -> [ok || X <- [BadMap], (R#options{enabled = map_size(X) > 0}) =:= R] end,
        fun() -> << <<1>> || (R#options{enabled = map_size(BadMap) > 0}) =:= R >> end,
        fun() -> << <<X>> || (R#options{enabled = map_size(BadMap) > 0}) =:= R, X <- [1] >> end,
        fun() -> << <<1>> || X <- [BadMap], (R#options{enabled = map_size(X) > 0}) =:= R >> end,
        fun() -> [X || <<X>> <= <<1>>, (R#options{enabled = map_size(BadMap) > 0}) =:= R] end,
        fun() -> << <<X>> || <<X>> <= <<1>>, (R#options{enabled = map_size(BadMap) > 0}) =:= R >> end,
        fun() -> [ok || is_tuple({R#options{enabled = map_size(BadMap) > 0}})] end,
        fun() -> [ok || #update_default{} =:= {update_default, R}] end
    ],
    lists:foreach(fun(F) -> {error, {badmap, not_a_map}} = result(F) end, Cases),
    [ok] = [ok || (R#options{enabled = false}) =/= R],
    <<1>> = << <<1>> || (R#options{enabled = false}) =/= R >>,
    ok.

result(Fun) -> try Fun() catch Class:Reason -> {Class, Reason} end.
event(Name, Value) -> put(trace, [Name | get(trace)]), Value.
default_enabled() -> error(default_failed).
length(X) -> erlang:length(X).
id_map() -> #{a => 1}.
id(X) -> X.
