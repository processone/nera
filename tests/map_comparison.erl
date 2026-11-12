%% EXPECT: map_comparison_ok
%% Regression: BEAM map comparison preserves exact map-key identity and ordering,
%% while ordinary map values retain numeric equality. Exercise shared callers.
-module(map_comparison).
-export([test/0]).

test() ->
    Pairs = id([
        {#{1=>a}, #{1.0=>a}, less, false},
        %% Every integer precedes every float within map keys, at every depth.
        {#{2=>a}, #{1.0=>a}, less, false},
        {#{-0.0=>a}, #{0.0=>a}, less, false},
        {#{{2}=>a}, #{{1.0}=>a}, less, false},
        {#{[2]=>a}, #{[1.0]=>a}, less, false},
        {#{#{k=>2}=>a}, #{#{k=>1.0}=>a}, less, false},
        {#{#{k=>1}=>a}, #{#{k=>1.0}=>a}, less, false},
        {#{{#{k=>1}}=>a}, #{{#{k=>1.0}}=>a}, less, false},
        {#{k=>1}, #{k=>1.0}, equal, false},
        {#{outer=>#{k=>1}}, #{outer=>#{k=>1.0}}, equal, false},
        {#{k=>2}, #{k=>1.0}, greater, false},
        %% Canonical key order must not depend on map insertion order.
        {#{1=>a,1.0=>b}, #{1.0=>b,1=>a}, equal, true},
        {#{1=>a,1.0=>b}, #{1.0=>a,1=>b}, less, false},
        %% All keys compare before any values.
        {#{1=>z,2=>a}, #{1=>a,3=>z}, less, false},
        %% The exact map-key mode must not leak into ordinary compounds.
        {2, 1.0, greater, false},
        {{2}, {1.0}, greater, false},
        {[2], [1.0], greater, false}
    ]),
    lists:foreach(fun({A,B,Order,Exact}) ->
        check_operators(A, B, Order, Exact),
        check_operators(B, A, reverse(Order), Exact),
        Equal = Order =:= equal,
        check_searches(A, B, Equal),
        check_searches(B, A, Equal)
    end, Pairs),
    ok = ordered_set_keys(),
    io:format("map_comparison_ok~n"),
    ok.

check_operators(A, B, Order, Exact) ->
    Equal = Order =:= equal,
    Different = not Equal,
    Less = Order =:= less,
    Greater = Order =:= greater,
    LessEqual = not Greater,
    GreaterEqual = not Less,
    Inexact = not Exact,
    Equal = (A == B),
    Different = (A /= B),
    Less = (A < B),
    Greater = (A > B),
    LessEqual = (A =< B),
    GreaterEqual = (A >= B),
    Exact = (A =:= B),
    Inexact = (A =/= B),
    ok.

check_searches(Query, Candidate, Equal) ->
    List = [{Candidate,value}],
    case Equal of
        true ->
            {Candidate,value} = lists:keyfind(Query, 1, List),
            true = lists:keymember(Query, 1, List),
            {value,{Candidate,value}} = lists:keysearch(Query, 1, List),
            {value,{Candidate,value},[]} = lists:keytake(Query, 1, List);
        false ->
            false = lists:keyfind(Query, 1, List),
            false = lists:keymember(Query, 1, List),
            false = lists:keysearch(Query, 1, List),
            false = lists:keytake(Query, 1, List)
    end,
    ok.

ordered_set_keys() ->
    A = id(#{1=>a}), B = id(#{1.0=>a}),
    T = ets:new(map_comparison_distinct, [ordered_set]),
    true = ets:insert(T, [{B,float_key},{A,integer_key}]),
    2 = ets:info(T, size),
    [{A,integer_key}] = ets:lookup(T, A),
    [{B,float_key}] = ets:lookup(T, B),
    A = ets:first(T),
    B = ets:next(T, A),
    '$end_of_table' = ets:next(T, B),
    true = ets:delete(T),
    C = id(#{id=>1}), D = id(#{id=>1.0}),
    U = ets:new(map_comparison_values, [ordered_set]),
    true = ets:insert(U, {C,old}),
    true = ets:insert(U, {D,new}),
    1 = ets:info(U, size),
    [{D,new}] = ets:lookup(U, C),
    [{D,new}] = ets:lookup(U, D),
    true = ets:delete(U),
    ok.

reverse(less) -> greater;
reverse(greater) -> less;
reverse(equal) -> equal.

id(X) -> X.
