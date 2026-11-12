%% EXPECT: lists_key_numeric_ok
%% Regression: keydelete/3, keyreplace/4 and keystore/4 must match keys with ==,
%% as on BEAM, rather than =:=. Run this same fixture on native Nera and BEAM.
-module(lists_key_numeric).
-export([test/0]).

test() ->
    %% Each match records the unchanged prefix and tail around the first match.
    Matches = id([
        {1.0, 1, [{1,a}], [], []},
        {1, 1, [{1.0,a}], [], []},
        {1.0, 1, [{1,a},{1.0,later}], [], [{1.0,later}]},
        {1, 2, [{before,9},{row,1.0},{after_row,2}], [{before,9}], [{after_row,2}]},
        {1.0, 2, [{before,9},{row,1}], [{before,9}], []},
        {{invoice,[1]}, 1, [{{invoice,[1.0]},a}], [], []},
        {0.0, 1, [{-0.0,a}], [], []},
        {ready, 1, [{pending,a},{ready,b}], [{pending,a}], []},
        {<<"id">>, 2, [{row,<<"id">>}], [], []},
        %% Map keys are exact, but corresponding map values still use ==.
        {#{id=>1}, 1, [{#{id=>1.0},a}], [], []},
        {#{id=>#{count=>1}}, 1, [{#{id=>#{count=>1.0}},a}], [], []},
        {#{1=>a,1.0=>b}, 1, [{#{1.0=>b,1=>a},v}], [], []},
        %% Guard failure skips values with no Nth tuple element.
        {1.0, 2, [ignored,{}, {short},[1],{row,1},{tail,2}],
         [ignored,{}, {short},[1]], [{tail,2}]}
    ]),
    Misses = id([
        {1.0, 1, []},
        {1.5, 1, [{1,a},{2,b}]},
        {1, 1, [{<<"1">>,text},{one,atom}]},
        {1.0, 2, [ignored,{}, {short},[1],{row,2}]},
        {#{1=>a}, 1, [{#{1.0=>a},v}]},
        {#{-0.0=>a}, 1, [{#{0.0=>a},v}]},
        {#{#{id=>1}=>a}, 1, [{#{#{id=>1.0}=>a},v}]},
        {#{1=>a,1.0=>b}, 1, [{#{1.0=>a,1=>b},v}]}
    ]),
    lists:foreach(fun(Mode) ->
        {Delete, Replace, Store} = calls(Mode),
        lists:foreach(fun({Key, N, List, Prefix, Tail}) ->
            Remaining = Prefix ++ Tail,
            NewTuple = {updated,value},
            Updated = Prefix ++ [NewTuple|Tail],
            Remaining = Delete(Key, N, List),
            Updated = Replace(Key, N, List, NewTuple),
            Updated = Store(Key, N, List, NewTuple)
        end, Matches),
        lists:foreach(fun({Key, N, List}) ->
            NewTuple = {updated,value},
            Appended = List ++ [NewTuple],
            List = Delete(Key, N, List),
            List = Replace(Key, N, List, NewTuple),
            Appended = Store(Key, N, List, NewTuple)
        end, Misses),
        %% Invalid positions must raise at the public entry, not become misses.
        lists:foreach(fun(N) ->
            lists:foreach(fun(List) ->
                function_clause = failure(fun() -> Delete(k, N, List) end),
                function_clause = failure(fun() -> Replace(k, N, List, {k,new}) end),
                function_clause = failure(fun() -> Store(k, N, List, {k,new}) end)
            end, [[],[{k,v}]])
        end, id([0,-1,1.0,position]))
    end, [direct, apply_call, dynamic, remote_fun]),
    io:format("lists_key_numeric_ok~n"),
    ok.

calls(direct) ->
    {fun(K, N, L) -> lists:keydelete(K, N, L) end,
     fun(K, N, L, T) -> lists:keyreplace(K, N, L, T) end,
     fun(K, N, L, T) -> lists:keystore(K, N, L, T) end};
calls(apply_call) ->
    {fun(K, N, L) -> apply(id(lists), id(keydelete), [K,N,L]) end,
     fun(K, N, L, T) -> apply(id(lists), id(keyreplace), [K,N,L,T]) end,
     fun(K, N, L, T) -> apply(id(lists), id(keystore), [K,N,L,T]) end};
calls(dynamic) ->
    M = id(lists), D = id(keydelete), R = id(keyreplace), S = id(keystore),
    {fun(K, N, L) -> M:D(K, N, L) end,
     fun(K, N, L, T) -> M:R(K, N, L, T) end,
     fun(K, N, L, T) -> M:S(K, N, L, T) end};
calls(remote_fun) ->
    {fun lists:keydelete/3, fun lists:keyreplace/4, fun lists:keystore/4}.

failure(F) ->
    try F() of Value -> {unexpected_return,Value}
    catch error:Reason -> Reason
    end.

id(X) -> X.
