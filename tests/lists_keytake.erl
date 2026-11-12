-module(lists_keytake).
-export([test/0]).

test() ->
    false = lists:keytake(a, 1, []),
    false = lists:keytake(absent, 1, [{a,1}, {b,2}]),
    {value,{a,1},[{b,2},{a,3}]} = lists:keytake(a, 1, [{a,1},{b,2},{a,3}]),
    {value,{b,2},[{a,1},{c,3}]} = lists:keytake(b, 1, [{a,1},{b,2},{c,3}]),
    {value,{c,3},[{a,1},{b,2}]} = lists:keytake(c, 1, [{a,1},{b,2},{c,3}]),
    {value,{x,a},[atom,{}, {a}, [a]]} = lists:keytake(a, 2, [atom,{}, {a},[a],{x,a}]),
    {value,{1.0,x},[]} = lists:keytake(1, 1, [{1.0,x}]),
    {value,{1,x},[]} = lists:keytake(1.0, 1, [{1,x}]),
    {value,{[1.0],x},[]} = lists:keytake([1], 1, [{[1.0],x}]),
    false = lists:keytake(a, 999999999999999999999999, [{a}]),
    {value,{a},tail} = lists:keytake(a, 1, [{a}|tail]),
    {value,{a},[x|tail]} = lists:keytake(a, 1, [x,{a}|tail]),
    function_clause = failure(a, 1, [x|tail]),
    function_clause = failure(a, 1, not_a_list),
    ok = invalid_positions([0,-1,1.0,bad]),
    {value,{a},[]} = apply(lists, keytake, [a,1,[{a}]]),
    F = fun lists:keytake/3,
    {value,{a},[]} = F(a,1,[{a}]),
    Prefix = lists:duplicate(20000, skipped),
    {value,{a},Prefix} = lists:keytake(a, 1, Prefix ++ [{a}]),
    ok.

invalid_positions([]) -> ok;
invalid_positions([N|Rest]) ->
    function_clause = failure(a, N, []),
    function_clause = failure(a, N, [{a}]),
    invalid_positions(Rest).

failure(K, N, L) ->
    try lists:keytake(K, N, L) of _ -> unexpected_success
    catch error:Reason -> Reason
    end.
