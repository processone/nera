%% EXPECT: process_dict_exact_keys_ok
%% NERA-87: dictionary lookup, replacement and erasure use exact key equality.
%% Run the same cases on native Nera and OTP 29; enumeration order is unspecified.
-module(process_dict_exact_keys).
-export([test/0]).

test() ->
    Big = id(1) bsl 100,
    Ref = make_ref(),
    F = fun id/1,
    Cases = [
        {1, 1.0, false},
        {0.0, -0.0, false},
        {Big, float(Big), false},
        {{child, [1]}, {child, [1.0]}, false},
        {[1, {child, 2}], [1.0, {child, 2}], false},
        {[child | 1], [child | 1.0], false},
        {#{state => 1}, #{state => 1.0}, false},
        {#{1 => active}, #{1.0 => active}, false},
        {{child, [0.0]}, {child, [-0.0]}, false},
        {#{state => 0.0}, #{state => -0.0}, false},
        {#{0.0 => active}, #{-0.0 => active}, false},
        {1, id(1), true},
        {1.0, id(1.0), true},
        {Big, id(1) bsl 100, true},
        {{child, [1]}, {id(child), [id(1)]}, true},
        {[child | 1], [id(child) | id(1)], true},
        {#{a => 1, b => active}, #{b => id(active), a => id(1)}, true},
        {<<"child">>, list_to_binary(id("child")), true},
        {<<1:3>>, <<(id(1)):3>>, true},
        {<<1:3>>, <<1:4>>, false},
        {self(), self(), true},
        {Ref, Ref, true},
        {Ref, make_ref(), false},
        {F, F, true}
    ],
    lists:foreach(fun(Mode) ->
        lists:foreach(fun({Key, Query, Same}) ->
            check_pair(Key, Query, Same, calls(Mode)),
            check_pair(Query, Key, Same, calls(Mode))
        end, Cases)
    end, [direct, qualified, apply_call, dynamic, local_fun, remote_fun]),
    io:format("process_dict_exact_keys_ok~n"),
    ok.

check_pair(Key, Query, Same, {Put, Get, Erase}) ->
    Same = Key =:= Query,
    [] = get(),
    undefined = Get(Key),
    undefined = Erase(Key),
    undefined = Put(Key, first),
    first = Get(Key),
    case Same of
        true ->
            first = Get(Query),
            first = Put(Query, updated),
            1 = length(get_keys()),
            updated = Get(Key),
            updated = Erase(Query),
            undefined = Get(Key);
        false ->
            %% A missing near-equal key must neither read nor erase Key.
            undefined = Get(Query),
            undefined = Erase(Query),
            [{Key, first}] = get(),
            undefined = Put(Query, second),
            2 = length(get_keys()),
            first = Get(Key),
            second = Get(Query),
            [Key] = get_keys(first),
            [Query] = get_keys(second),
            second = Put(Query, updated),
            2 = length(get()),
            first = Get(Key),
            updated = Get(Query),
            updated = Erase(Query),
            undefined = Get(Query),
            [{Key, first}] = get(),
            first = Erase(Key)
    end,
    undefined = Erase(Query),
    [] = get(),
    [] = get_keys(),
    ok.

calls(direct) ->
    {fun(K, V) -> put(K, V) end,
     fun(K) -> get(K) end,
     fun(K) -> erase(K) end};
calls(qualified) ->
    {fun(K, V) -> erlang:put(K, V) end,
     fun(K) -> erlang:get(K) end,
     fun(K) -> erlang:erase(K) end};
calls(apply_call) ->
    {fun(K, V) -> apply(id(erlang), id(put), [K, V]) end,
     fun(K) -> apply(id(erlang), id(get), [K]) end,
     fun(K) -> apply(id(erlang), id(erase), [K]) end};
calls(dynamic) ->
    M = id(erlang),
    P = id(put), G = id(get), E = id(erase),
    {fun(K, V) -> M:P(K, V) end,
     fun(K) -> M:G(K) end,
     fun(K) -> M:E(K) end};
calls(local_fun) -> {fun put/2, fun get/1, fun erase/1};
calls(remote_fun) -> {fun erlang:put/2, fun erlang:get/1, fun erlang:erase/1}.

id(X) -> X.
