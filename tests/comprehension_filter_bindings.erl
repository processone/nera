%% EXPECT: comprehension_filter_bindings_ok
%% Run the same assertions natively and on BEAM. Filter bindings must cross
%% helper boundaries without repeating the filter or leaking between iterations.
-module(comprehension_filter_bindings).
-export([test/0]).

test() ->
    check(requested, [258],
        [V || X <- [16], begin N = X, true end, <<V:N>> <= <<1, 2>>]),
    check(binary_outer_list_body, [1, 2, 258],
        [V || <<X>> <= id(<<8, 16>>), begin N = X, true end,
            <<V:N>> <= id(<<1, 2>>)]),
    check(binary_outer_binary_body, <<1, 2, 1, 2>>,
        << <<V:N>> || <<X>> <= id(<<8, 16>>), begin N = X, true end,
            <<V:N>> <= id(<<1, 2>>) >>),
    check(list_source, [11, 12, 21, 22],
        [V || X <- id([10, 20]), begin Values = [X + 1, X + 2], true end,
            V <- Values]),
    check(binary_source, [1, 2, 3, 4],
        [V || X <- id([1, 3]), begin Bits = <<X, (X + 1)>>, true end,
            <<V>> <= Bits]),
    check(list_pattern_size, [1, 258],
        [V || X <- id([8, 16]), begin N = X, true end,
            <<V:N>> <- id([<<1>>, <<1, 2>>, <<1, 2, 3>>])]),
    check(binary_pattern_size, [1, 2, 258],
        [V || X <- id([8, 16]), begin N = X, true end,
            <<V:N>> <= id(<<1, 2>>)]),
    check(expression_size, [1, 2, 258],
        [V || X <- id([8, 16]), begin N = X div 8, true end,
            <<V:(N * 8)>> <= id(<<1, 2>>)]),
    check(map_key, [1, 2],
        [V || X <- id([a, b]), begin K = X, true end,
            #{K := V} <- id([#{a => 1}, #{b => 2}])]),
    check(nested_pattern, [1, 2],
        [V || X <- id([a, b]), begin K = X, true end,
            {tag, [#{K := V}]} <- id([{tag, [#{a => 1}]}, {tag, [#{b => 2}]}])]),
    check(template, [{10, 1}, {10, 2}, {20, 1}, {20, 2}],
        [{N, Y} || X <- id([10, 20]), begin N = X, true end, Y <- id([1, 2])]),
    check(three_generators, [{8, 1, a}, {8, 1, b}, {8, 2, a}, {8, 2, b},
            {16, 258, a}, {16, 258, b}],
        [{N, V, Y} || X <- id([8, 16]), begin N = X, true end,
            <<V:N>> <= id(<<1, 2>>), Y <- id([a, b])]),
    check(two_filters, [{10, 11}, {20, 21}],
        [{N, M} || X <- id([10, 20]), begin N = X, true end,
            Y <- id([1]), begin M = N + Y, true end, _ <- id([ok])]),
    check(later_filter, [11, 21],
        [M || X <- id([10, 20]), begin N = X, true end,
            Y <- id([1, 2]), begin M = N + Y, Y =:= 1 end, _ <- id([ok])]),
    check(shadow_size, [{258, 7}, {258, 9}],
        [{V, N} || _ <- id([ok]), begin N = id(16), true end,
            <<V:N, N:8>> <- id([<<1, 2, 7>>, <<1, 2, 9>>]), _ <- id([ok])]),
    check(resume_shadowed, [{8, 1, 7}, {8, 2, 7}, {16, 258, 7}],
        [{X, V, N} || X <- id([8, 16]), begin N = X, true end,
            <<V:N>> <= id(<<1, 2>>), N <- id([7]), _ <- id([ok])]),
    check(branch_export, [1, 2, 258],
        [V || X <- id([8, 16]),
            case X of 8 -> N = id(8), true; 16 -> N = id(16), true end,
            <<V:N>> <= id(<<1, 2>>)]),
    check(closure_template, [11, 12, 21, 22],
        [(fun() -> N + Y end)() || X <- id([10, 20]), begin N = X, true end,
            Y <- id([1, 2])]),
    check(closure_binding, [11, 12, 21, 22],
        [F(Y) || X <- id([10, 20]), begin F = fun(Y) -> X + Y end, true end,
            Y <- id([1, 2])]),
    check(binary_template, <<1, 2, 1, 2>>,
        << <<V:N>> || X <- id([8, 16]), begin N = X, true end,
            <<V:N>> <= id(<<1, 2>>) >>),
    check(binary_map_key, <<1, 2>>,
        << <<V>> || X <- id([a, b]), begin K = X, true end,
            #{K := V} <- id([#{a => 1}, #{b => 2}]) >>),
    check(binary_three_generators, <<1, 2, 1, 2>>,
        << <<V:N>> || X <- id([8, 16]), begin N = X, true end,
            <<V:N>> <= id(<<1, 2>>), _ <- id([ok]) >>),
    check(deferred_tail, [<<5:3>>, <<6:3>>],
        [Tail || B <- id([<<0, 5:3>>, <<0, 6:3>>]),
            begin <<0, Tail/bits>> = B, true end, _ <- id([ok]), _ <- id([ok])]),
    check(tail_as_key, [found],
        [V || B <- id([<<0, 5:3>>]), begin <<0, Key/bits>> = B, true end,
            #{Key := V} <- id([#{<<5:3>> => found}])]),
    Outer = id(2),
    check(bound_filter_match, [2],
        [Outer || X <- id([2]), begin Outer = X, true end, _ <- id([ok])]),
    check(no_leak, [local],
        [Local || _ <- id([ok]), begin Local = id(local), true end, _ <- id([ok])]),
    Local = id(outside),
    check(outside_binding, outside, Local),
    traces(),
    io:format("comprehension_filter_bindings_ok~n"),
    ok.

traces() ->
    check(filter_once, {[11, 12, 21, 22],
            [{filter, 10}, {source, 10}, {body, 11}, {body, 12},
             {filter, 20}, {source, 20}, {body, 21}, {body, 22}]},
        traced(fun() ->
            [event(body, N + Y) || X <- id([10, 20]),
                begin N = event(filter, X), true end,
                Y <- event(source, [1, 2], N)]
        end)),
    check(skip_false, {[2], [{filter, 1}, {filter, 2}, {source, 2}, {body, 2}]},
        traced(fun() ->
            [event(body, N) || X <- id([1, 2]),
                begin N = event(filter, X), X =:= 2 end,
                _ <- event(source, [ok], N)]
        end)),
    check(empty_inner, {[2], [{filter, 1}, {source, 1},
            {filter, 2}, {source, 2}, {body, 2}]},
        traced(fun() ->
            [event(body, N) || X <- id([1, 2]), begin N = event(filter, X), true end,
                _ <- event(source, case N of 1 -> []; 2 -> [ok] end, N)]
        end)),
    ok.

traced(Fun) ->
    put(trace, []),
    Result = Fun(),
    {Result, lists:reverse(erase(trace))}.

event(Name, Value) -> event(Name, Value, Value).
event(Name, Value, Logged) -> put(trace, [{Name, Logged} | get(trace)]), Value.

check(Name, Expected, Actual) ->
    io:format("~p: ~p~n", [Name, Actual]),
    Expected = Actual,
    ok.

id(X) -> X.
