%% EXPECT: atom_order_ok
%% Regression: atoms compare by their names, as on BEAM, whatever order they
%% were created in. Natively they compared by creation order, so zebra < apple
%% was true, lists:sort([zebra, apple, mango]) returned its input, and a sorted
%% key list of Cowlib's server-sent event map came out reversed. BEAM orders
%% atoms by their UTF-8 names, byte by byte, a name before every longer name
%% that starts with it; for character lists that is lists:sort/1's order.
%% Atoms are first mentioned here in reverse order, and more are created at
%% run time in reverse order. Run this same fixture on native Nera and BEAM.
-module(atom_order).
-export([test/0]).

test() ->
    %% zebra is mentioned before mango and apple.
    Zebra = id(zebra), Mango = id(mango), Apple = id(apple),
    false = Zebra < Apple,
    true = Apple < Zebra,
    true = Zebra > Mango,
    true = Mango >= Apple,
    false = Zebra =< Apple,
    true = Apple =< id(apple),
    true = Apple == id(apple),
    true = Zebra /= Apple,
    [apple, mango, zebra] = lists:sort([Zebra, Apple, Mango]),
    [apple, mango, zebra] = lists:usort([Zebra, Apple, Zebra, Mango, Apple]),
    [zebra, mango, apple] = lists:sort(fun(A, B) -> A >= B end, [Apple, Zebra, Mango]),
    zebra = lists:max([Apple, Zebra, Mango]),
    apple = lists:min([Mango, Zebra, Apple]),
    before_mango = guard(Apple),
    from_mango = guard(Mango),
    from_mango = guard(Zebra),

    %% Atoms created at run time, last name first.
    Words = ["atom_order_zulu", "atom_order_yankee", "atom_order_mike",
             "atom_order_golf", "atom_order_alpha"],
    check_order(Words),
    [AZulu, AYankee, AMike, AGolf, AAlpha] = [list_to_atom(W) || W <- Words],
    true = AAlpha < AZulu,
    false = AZulu < AYankee,
    true = id(apple) < AAlpha,
    %% Prefixes and lengths: a name sorts before the longer names it starts,
    %% across the first eight bytes and beyond them.
    check_order(["b", "ab", "aa", "a", ""]),
    check_order(["abcdefgi", "abcdefgh_z", "abcdefgh_a", "abcdefgh_", "abcdefgh"]),
    check_order(["atom_order_long_name_z", "atom_order_long_name_a", "atom_order_long_name"]),
    %% Case and punctuation follow character codes: "Z" < "_" < "a".
    check_order(["zebra", "apple", "_apple", "Zebra", "Apple"]),
    true = 'Zebra' < apple,
    %% Characters above 127: UTF-8 bytes order as code points do.
    check_order([[16#1F600], [16#4E2D], [16#3B1], [16#100], [16#FF], [16#E9], "z"]),
    check_order([[$a, 16#100], [$a, 16#FF], [$a, 16#E9, $z], [$a, 16#E9], "az"]),

    %% Compound terms compare atoms the same way.
    true = {Zebra, 1} > {Apple, 2},
    true = [Zebra] > [Apple, extra],
    true = {AZulu} > {AAlpha},
    [{apple, 2}, {mango, 3}, {zebra, 1}] =
        lists:sort(fun({K1, _}, {K2, _}) -> K1 =< K2 end,
                   [{Zebra, 1}, {Apple, 2}, {Mango, 3}]),
    [{AAlpha, 5}, {AMike, 3}, {AZulu, 1}] =
        lists:sort([{AZulu, 1}, {AMike, 3}, {AAlpha, 5}]),
    %% cow_sse:parse/2 returns this event; its keys sorted as on BEAM.
    Event = #{last_event_id => <<>>, event_type => <<"message">>, data => [<<"hi">>]},
    [{data, [<<"hi">>]}, {event_type, <<"message">>}, {last_event_id, <<>>}] =
        lists:sort(maps:to_list(Event)),
    [data, event_type, last_event_id] = lists:sort(maps:keys(Event)),
    true = data < event_type,
    true = event_type < last_event_id,

    %% Maps with the same keys compare values in key order: apple's first.
    true = #{zebra => 1, apple => 9} > #{zebra => 2, apple => 0},
    true = maps:from_list([{AZulu, 1}, {AAlpha, 9}]) > maps:from_list([{AZulu, 2}, {AAlpha, 0}]),
    %% Maps with different keys compare their sorted keys first.
    true = #{apple => 1} < #{zebra => 0},
    true = #{AAlpha => 1, AZulu => 1} < #{AMike => 0, AZulu => 0},

    %% erlang:max/2 and min/2 use the same order, and the term order between
    %% types: number < atom < tuple < list.
    zebra = max(Zebra, Apple),
    zebra = max(Apple, Zebra),
    apple = min(Zebra, Apple),
    apple = min(Apple, Zebra),
    AZulu = max(AGolf, AZulu),
    AAlpha = min(AYankee, AAlpha),
    zebra = max(1, Zebra),
    1 = min(Zebra, 1),
    {apple} = max(Zebra, {apple}),
    zebra = min({apple}, Zebra),
    "a" = max(Zebra, "a"),
    zebra = apply(erlang, max, [Apple, Zebra]),
    Min = fun erlang:min/2,
    apple = Min(Zebra, Apple),
    [apple, mango, zebra] = apply(lists, sort, [[Zebra, Mango, Apple]]),

    %% An ordered_set table iterates its atom keys in name order.
    T = ets:new(atom_order, [ordered_set]),
    true = ets:insert(T, [{Zebra, 1}, {AZulu, 2}, {Apple, 3}, {AAlpha, 4}, {Mango, 5}]),
    apple = ets:first(T),
    zebra = ets:last(T),
    AAlpha = ets:next(T, apple),
    mango = ets:prev(T, zebra),
    AZulu = ets:prev(T, mango),
    [apple, AAlpha, AZulu, mango, zebra] = [K || {K, _} <- ets:tab2list(T)],
    true = ets:delete(T),

    %% Equality is unchanged.
    true = Zebra =:= list_to_atom("zebra"),
    true = AZulu =:= binary_to_atom(<<"atom_order_zulu">>, utf8),
    false = Zebra =:= Apple,
    io:format("atom_order_ok~n"),
    ok.

%% Atoms made from Names sort as Names do.
check_order(Names) ->
    Atoms = [list_to_atom(N) || N <- Names],
    Expected = [list_to_atom(N) || N <- lists:sort(Names)],
    Expected = lists:sort(Atoms),
    Expected = lists:sort(lists:reverse(Atoms)),
    [First | _] = Expected,
    First = lists:min(Atoms),
    Pairs = [{A, B} || A <- Atoms, B <- Atoms],
    true = lists:all(fun({A, B}) ->
                             (A < B) =:= (atom_to_list(A) < atom_to_list(B))
                     end, Pairs),
    ok.

guard(X) when X < mango -> before_mango;
guard(_) -> from_mango.

id(X) -> X.
