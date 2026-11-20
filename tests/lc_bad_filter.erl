%% EXPECT: lc_bad_filter_ok
%% Regression: a comprehension filter that is not a guard test and returns
%% neither true nor false raises error:{bad_filter, Value} as on BEAM, in list
%% and binary comprehensions. Before the fix, Nera skipped the element:
%% [X || X <- id([1, 0]), X > 0, id(1 div X) > 0, self() ! later] returned []
%% instead of raising {bad_filter, later}.
%% A filter that is a guard test keeps guard rules: any value but true skips
%% the element. Elements before the bad one are still produced; later
%% filters, bodies and elements do not run.
%% Run this same fixture on native Nera and BEAM.
-module(lc_bad_filter).
-export([test/0]).
-compile(nowarn_deprecated_catch).

-record(r, {a = 1}).

test() ->
    lists:foreach(fun({Name, Expected, Fun}) ->
        case result(Fun) of
            Expected -> ok;
            Actual -> erlang:error({wrong_result, Name, Expected, Actual})
        end
    end, cases()),
    traces(),
    try_bodies(),
    uncaught(),
    io:format("lc_bad_filter_ok~n"),
    ok.

cases() ->
    reported() ++ values() ++ shapes() ++ guards() ++ valid().

%% The reported case: the send's value, the atom later, is the bad value.
reported() ->
    [{send, bad(later), fun() ->
        R = [X || X <- id([1, 0]), X > 0, id(1 div X) > 0, self() ! later],
        receive later -> R end
     end},
     {send_caught, {later, bad(later)}, fun() ->
        R = result(fun() -> [X || X <- id([1]), self() ! later] end),
        receive later -> {later, R} end
     end}].

%% Every kind of value that is not a boolean is reported unchanged.
values() ->
    Pid = self(),
    Fun = fun id/1,
    [{atom, bad(nope), fun() -> [X || X <- id([1]), id(nope)] end},
     {integer, bad(1), fun() -> [X || X <- id([1, 2]), id(X)] end},
     {big_integer, bad(1 bsl 70), fun() -> [X || X <- id([1]), id(1 bsl 70)] end},
     {float, bad(1.5), fun() -> [X || X <- id([1]), id(1.5)] end},
     {tuple, bad({true}), fun() -> [X || X <- id([1]), id({true})] end},
     {list, bad([true]), fun() -> [X || X <- id([1]), id([true])] end},
     {empty_list, bad([]), fun() -> [X || X <- id([1]), id([])] end},
     {map, bad(#{a => 1}), fun() -> [X || X <- id([1]), id(#{a => 1})] end},
     {binary, bad(<<"true">>), fun() -> [X || X <- id([1]), id(<<"true">>)] end},
     {pid, bad(Pid), fun() -> [X || X <- id([1]), id(Pid)] end},
     {function, bad(Fun), fun() -> [X || X <- id([1]), id(Fun)] end}].

%% Each filter is checked where it runs.
shapes() ->
    [{binary_comprehension, bad(1), fun() -> << <<X>> || X <- id([1, 2]), id(X) >> end},
     {binary_generator, bad(nope), fun() -> [X || <<X>> <= id(<<1, 2>>), id(nope)] end},
     {binary_both, bad(1), fun() -> << <<X>> || <<X>> <= id(<<1, 2>>), id(X) >> end},
     {outer_binary_generator, bad(1), fun() -> [{X, Y} || <<X>> <= id(<<1, 2>>), id(X), Y <- id([a])] end},
     {inner_generator, bad(a), fun() -> [{X, Y} || X <- id([1, 2]), Y <- id([a, b]), id(Y)] end},
     {outer_generator, bad(1), fun() -> [{X, Y} || X <- id([1, 2]), id(X), Y <- id([a])] end},
     {after_true, bad(1), fun() -> [X || X <- id([1, 2]), id(true), id(X)] end},
     {after_guard, bad(2), fun() -> [X || X <- id([1, 2, 3]), X > 1, id(X)] end},
     {leading, bad(later), fun() -> [X || id(later), X <- id([1])] end},
     {leading_only, bad(later), fun() -> [x || id(later)] end},
     {leading_binary, bad(later), fun() -> << <<X>> || id(later), X <- id([1]) >> end},
     {leading_only_binary, bad(later), fun() -> << <<1>> || id(later) >> end},
     {record_update, bad(#r{a = 2}), fun() -> [X || X <- id([1]), (id(#r{}))#r{a = 2}] end},
     {andalso_right, bad(later), fun() -> [X || X <- id([1]), id(true) andalso id(later)] end},
     {orelse_right, bad(later), fun() -> [X || X <- id([1]), id(false) orelse id(later)] end},
     {case_value, bad(nope), fun() -> [X || X <- id([1, 2]), case X of 1 -> true; _ -> nope end] end},
     {block_value, bad(later), fun() -> [X || X <- id([1]), begin Y = id(later), Y end] end}].

%% A guard test that is not true is false: the element is skipped.
guards() ->
    [{guard_variable, [true], fun() -> [X || X <- id([a, true, 1]), X] end},
     {guard_arithmetic, [], fun() -> [X || X <- id([1, 2]), X + 1] end},
     {guard_element, [{true}], fun() -> [T || T <- id([{a}, {true}]), element(1, T)] end},
     {guard_andalso_binary, <<2>>, fun() -> << <<X>> || <<X>> <= id(<<1, 2>>), X =:= 2 orelse X >> end},
     {guard_leading, [], fun() -> L = id(later), [X || L, X <- id([1])] end}].

%% Filters that return booleans keep and skip elements as before; a false
%% filter stops before any later, non-boolean filter.
valid() ->
    [{boolean_call, [1, 3], fun() -> [X || X <- id([1, 2, 3]), id(X rem 2 =:= 1)] end},
     {comparison_of_call, [2, 3], fun() -> [X || X <- id([1, 2, 3]), id(X) > 1] end},
     {false_first, [], fun() -> [X || X <- id([1, 2]), id(false), id(X)] end},
     {false_guard_first, [2], fun() -> [X || X <- id([1, 2]), X > 1 orelse X =:= 3, id(X =:= 2)] end},
     {binary_boolean, <<2>>, fun() -> << <<X>> || <<X>> <= id(<<1, 2>>), id(X > 1) >> end},
     {leading_false, [], fun() -> [X || id(false), X <- id([1])] end},
     {leading_true, [1], fun() -> [X || id(true), X <- id([1])] end}].

%% Elements before the bad one run their filters and bodies; the bad filter
%% is the last thing that runs.
traces() ->
    {{error, {bad_filter, oops}}, [{filter, 1}, {body, 1}, {filter, 2}]} =
        traced(fun() -> [trace(X) || X <- id([1, 2, 3]), filter(X)] end),
    {{error, {bad_filter, oops}}, [{filter, 1}, {body, 1}, {filter, 2}]} =
        traced(fun() -> << <<(trace(X))>> || X <- id([1, 2, 3]), filter(X) >> end),
    {{error, {bad_filter, oops}}, [{filter, 2}]} =
        traced(fun() -> [trace(X) || X <- id([2]), filter(X), filter(1)] end),
    {{error, {bad_filter, oops}}, [{filter, 2}]} =
        traced(fun() -> [trace(X) || filter(2), X <- id([1])] end),
    ok.

%% Inside a try, the error skips the rest of the try body; catch returns it
%% with its stack trace.
try_bodies() ->
    caught = try
        _ = [X || X <- id([1]), id(oops)],
        self() ! leaked
    catch error:{bad_filter, oops} -> caught
    end,
    caught = try
        _ = [X || id(oops), X <- id([1])],
        self() ! leaked
    catch error:{bad_filter, oops} -> caught
    end,
    {'EXIT', {{bad_filter, oops}, Stack}} = (catch [X || X <- id([1]), id(oops)]),
    true = is_list(Stack),
    receive leaked -> erlang:error(leaked) after 0 -> ok end.

%% Outside any try, the error ends the process with the error and its stack
%% trace as the exit reason.
uncaught() ->
    {Pid, Ref} = spawn_monitor(fun() -> [X || X <- id([1]), id(oops)] end),
    receive
        {'DOWN', Ref, process, Pid, {{bad_filter, oops}, Stack}} when is_list(Stack) -> ok;
        {'DOWN', Ref, process, Pid, Other} -> erlang:error({wrong_exit, Other})
    after 5000 -> erlang:error(no_down)
    end.

bad(Value) -> {error, {bad_filter, Value}}.

traced(Fun) ->
    put(trace, []),
    Result = result(Fun),
    {Result, lists:reverse(erase(trace))}.

result(Fun) ->
    try Fun() catch Class:Reason -> {Class, Reason} end.

trace(X) -> put(trace, [{body, X} | get(trace)]), X.

filter(X) ->
    put(trace, [{filter, X} | get(trace)]),
    case X of
        2 -> oops;
        _ -> true
    end.

id(X) -> X.
