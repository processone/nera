%% EXPECT: fun_equality_ok
%% Same-expression closures compare by code and captures, as on BEAM.
%% This fixture owns closure equality and its dictionary/map/ETS key contract.
-module(fun_equality).
-export([test/0]).

test() ->
    A = make_adder(id(1)), B = make_adder(id(1)),
    3 = A(2), 3 = B(2),
    Ref = make_ref(),
    Pairs = [
        {A, B, true, true},
        {A, make_adder(id(2)), false, false},
        {A, other_adder(id(1)), false, false},
        {A, make_adder(id(1.0)), true, false},
        {capture(id(0.0)), capture(id(-0.0)), true, false},
        {capture(id(1.5)), capture(id(1.5)), true, true},
        {capture(id(1) bsl 100), capture(id(1) bsl 100), true, true},
        {capture(id(1) bsl 100), capture(-(id(1) bsl 100)), false, false},
        {capture({[id(child)], <<(id(5)):3>>, Ref, self()}),
         capture({[id(child)], <<(id(5)):3>>, Ref, self()}), true, true},
        {capture(#{a => id(1), b => [id(2.5)]}),
         capture(#{b => [id(2.5)], a => id(1)}), true, true},
        {capture(#{a => id(1)}), capture(#{a => id(1.0)}), true, false},
        {capture(#{id(1) => value}), capture(#{id(1.0) => value}), false, false},
        {capture(#{id(1) => integer, id(1.0) => float}),
         capture(#{id(1.0) => float, id(1) => integer}), true, true},
        {capture(#{A => first, make_adder(id(2)) => second}),
         capture(#{make_adder(id(2)) => second, B => first}), true, true},
        {capture(A), capture(B), true, true},
        {capture(A), capture(make_adder(id(2))), false, false},
        {two_captures(id(1), [id(2)]), two_captures(id(1), [id(2)]), true, true},
        {two_captures(id(1), [id(2)]), two_captures(id(1), [id(3)]), false, false},
        {named_capture([id(1)]), named_capture([id(1)]), true, true},
        {no_capture(), no_capture(), true, true},
        {remote_ref(), remote_ref(), true, true}
    ],
    lists:foreach(fun({F, G, Equal, Exact}) ->
        check_pair(F, G, Equal, Exact),
        check_pair(G, F, Equal, Exact)
    end, Pairs),
    %% Collection and message copying must not change a fun's identity.
    Self = self(),
    spawn(fun() -> Self ! {copied, capture({[id(7)], make_adder(id(1))})} end),
    receive
        {copied, Copied} ->
            Local = capture({[id(7)], make_adder(id(1))}),
            true = erlang:garbage_collect(),
            check_pair(Copied, Local, true, true)
    end,
    io:format("fun_equality_ok~n"),
    ok.

check_pair(F, G, Equal, Exact) ->
    Equal = (F == G),
    Exact = (F =:= G),
    NotEqual = not Equal, NotExact = not Exact,
    NotEqual = (F /= G), NotExact = (F =/= G),
    Equal = equal_guard(F, G), Exact = exact_guard(F, G),
    %% Exact identity must also survive nesting and pattern matching.
    Exact = ({[F]} =:= {[G]}),
    case Exact of
        true ->
            F = G,
            false = (F < G), false = (F > G),
            true = (F =< G), true = (F >= G);
        false -> ok
    end,
    dictionary_keys(F, G, Exact),
    map_keys(F, G, Exact),
    lists:foreach(fun(Type) -> ets_keys(Type, F, G, Exact) end,
                  [set, bag, duplicate_bag]),
    %% ordered_set uses ordinary term ordering, including numeric captures.
    ets_keys(ordered_set, F, G, Equal),
    ok.

dictionary_keys(F, G, Same) ->
    undefined = put(F, first),
    case Same of
        true ->
            first = get(G), first = put(G, updated),
            updated = get(F), updated = erase(G);
        false ->
            undefined = get(G), undefined = erase(G), first = erase(F)
    end,
    undefined = get(F),
    ok.

map_keys(F, G, Same) ->
    M = #{F => first},
    case Same of
        true ->
            first = maps:get(G, M),
            Updated = M#{G := updated},
            1 = map_size(Updated), updated = maps:get(F, Updated);
        false ->
            false = maps:is_key(G, M),
            Both = M#{G => second},
            2 = map_size(Both), first = maps:get(F, Both),
            second = maps:get(G, Both)
    end,
    ok.

ets_keys(Type, F, G, Same) ->
    T = ets:new(fun_equality, [Type]),
    true = ets:insert(T, {F, first}),
    case Same of
        true ->
            [{Stored, first}] = ets:lookup(T, G),
            %% Even an ordered_set returns the originally stored fun.
            true = (Stored =:= F),
            true = ets:member(T, G),
            false = ets:insert_new(T, {G, second}),
            true = ets:delete(T, G), [] = ets:lookup(T, F);
        false ->
            [] = ets:lookup(T, G), false = ets:member(T, G),
            true = ets:insert_new(T, {G, second}),
            2 = ets:info(T, size),
            [{Stored, first}] = ets:lookup(T, F), true = (Stored =:= F),
            true = ets:delete(T, G),
            [{Remaining, first}] = ets:lookup(T, F), true = (Remaining =:= F)
    end,
    true = ets:delete(T),
    ok.

equal_guard(F, G) when F == G -> true;
equal_guard(_, _) -> false.
exact_guard(F, G) when F =:= G -> true;
exact_guard(_, _) -> false.
make_adder(X) -> fun(Y) -> X + Y end.
other_adder(X) -> fun(Y) -> X + Y end.
capture(X) -> fun() -> X end.
two_captures(X, Y) -> fun() -> {X, Y} end.
named_capture(X) -> fun Loop(0) -> X; Loop(N) -> Loop(N - 1) end.
no_capture() -> fun() -> ok end.
remote_ref() -> fun erlang:abs/1.
id(X) -> X.
