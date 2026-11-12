%% EXPECT: process_dict_get_keys_ok
%% NERA-67: Ranch 1.8.1 ranch_conns_sup:249 selects children with get_keys(active).
%% The same assertions run natively and on BEAM; result order is unspecified.
-module(process_dict_get_keys).
-export([test/0]).

test() ->
    [] = get_keys(active),
    [] = erlang:get_keys(active),
    Ref = make_ref(),
    F = fun id/1,
    Big = id(1 bsl 100),
    Cases = [
        {active, active, true},
        {active, removed, false},
        {undefined, undefined, true},
        {[], [], true},
        {[], [active], false},
        {1, 1, true},
        {1.0, 1.0, true},
        {1, 1.0, false},
        {1.0, 1, false},
        {0.0, 0.0, true},
        {-0.0, -0.0, true},
        {0.0, -0.0, false},
        {-0.0, 0.0, false},
        {{child, [0.0]}, {child, [-0.0]}, false},
        {#{state => 0.0}, #{state => -0.0}, false},
        {#{0.0 => active}, #{-0.0 => active}, false},
        {Big, id(1) bsl 100, true},
        {Big, float(Big), false},
        {{child, [1, #{state => active}]}, {child, id([1, #{state => active}])}, true},
        {{child, [1]}, {child, [1.0]}, false},
        {[1, {child, 2}], [1.0, {child, 2}], false},
        {[active|removed], [id(active)|removed], true},
        {#{a => 1, b => active}, #{b => active, a => id(1)}, true},
        {#{a => 1}, #{a => 1.0}, false},
        {#{1 => active}, #{1.0 => active}, false},
        {<<"active">>, list_to_binary(id("active")), true},
        {<<1:3>>, <<1:3>>, true},
        {<<1:3>>, <<1:4>>, false},
        {self(), self(), true},
        {Ref, Ref, true},
        {Ref, make_ref(), false},
        {F, F, true}
    ],
    check_cases(Cases),
    %% NERA-88 F1: literal operands also exercise compile-time folding.
    false = (0.0 =:= -0.0),
    true = (0.0 =/= -0.0),
    true = (0.0 == -0.0),
    false = (0.0 /= -0.0),
    literal_false = case 0.0 =:= -0.0 of
        true -> literal_true;
        false -> literal_false
    end,
    %% Value filtering is exact; ordinary numeric equality is unchanged.
    true = id(0.0) == id(-0.0),
    true = id(1) == id(1.0),
    %% Compare separately allocated refc binaries and sub-binaries too.
    Large = list_to_binary(lists:duplicate(80, $a)),
    LargeCopy = list_to_binary(lists:duplicate(id(80), $a)),
    check_cases([{Large, LargeCopy, true},
                 {binary:part(Large, 1, 70), binary:part(LargeCopy, 2, 70), true}]),
    ranch_children(Ref),
    io:format("process_dict_get_keys_ok~n"),
    ok.

check_cases([]) -> ok;
check_cases([{Stored, Query, Matches}|Rest]) ->
    Matches = Stored =:= Query,
    Different = not Matches,
    Different = Stored =/= Query,
    Key = {get_keys_test, <<"child">>},
    undefined = put(Key, Stored),
    Before = lists:sort(get()),
    KeysBefore = lists:sort(get_keys()),
    Expected = case Matches of true -> [Key]; false -> [] end,
    Expected = get_keys(Query),
    Expected = erlang:get_keys(Query),
    Expected = apply(id(erlang), id(get_keys), [Query]),
    M = id(erlang),
    Name = id(get_keys),
    Expected = M:Name(Query),
    F = fun erlang:get_keys/1,
    Expected = F(Query),
    G = fun get_keys/1,
    Expected = G(Query),
    %% Neither enumerator may change dictionary entries, including undefined.
    Before = lists:sort(get()),
    KeysBefore = lists:sort(get_keys()),
    Stored = erase(Key),
    check_cases(Rest).

ranch_children(Ref) ->
    %% Ranch stores child pids as keys and active/removed as their state.
    Parent = self(),
    undefined = put({child, 2}, active),
    Child = spawn(fun() ->
        [] = get_keys(active),
        receive continue -> ok end,
        [] = get_keys(active),
        undefined = put(self(), active),
        [ChildKey] = get_keys(active),
        true = ChildKey =:= self(),
        Parent ! {Ref, child_ok}
    end),
    undefined = put(Child, active),
    undefined = put(<<"child">>, removed),
    Child ! continue,
    receive {Ref, child_ok} -> ok after 5000 -> error(child_timeout) end,
    Expected = lists:sort([Child, {child, 2}]),
    Expected = lists:sort(get_keys(active)),
    [<<"child">>] = get_keys(removed),
    active = put(Child, removed),
    [{child, 2}] = get_keys(active),
    Removed = lists:sort([Child, <<"child">>]),
    Removed = lists:sort(erlang:get_keys(removed)),
    removed = erase(Child),
    removed = erase(<<"child">>),
    active = erase({child, 2}),
    [] = get_keys(active),
    [] = get_keys(removed),
    ok.

id(X) -> X.
