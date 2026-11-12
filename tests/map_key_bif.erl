%% EXPECT: map_key_bif_ok
%% NERA-44: cow_ws:142 needs auto-imported is_map_key/2 under not.
%% Run the same assertions natively and with erlc/erl (BEAM).
-module(map_key_bif).
-export([test/0]).

test() ->
    Cases = [
        {client_max_window_bits_set, #{}, false},
        {client_max_window_bits_set, #{client_max_window_bits_set => true}, true},
        {client_max_window_bits_set, #{client_max_window_bits_set => false}, true},
        {missing, #{a => undefined}, false},
        {a, #{a => undefined}, true},
        {1, #{1 => integer}, true},
        {1.0, #{1 => integer}, false},
        {1, #{1.0 => float}, false},
        {1.0, #{1.0 => float}, true},
        {list_to_binary(id("window")), #{<<"window">> => 15}, true},
        {{option, id([1,2])}, #{{option, [1,2]} => 15}, true},
        {{option, id([1.0,2])}, #{{option, [1,2]} => 15}, false},
        {id([a,b]), #{[a,b] => 15}, true}
    ],
    check_maps(Cases),
    check_badmaps([not_a_map, 42, [], [a], {a}, <<"a">>, 1.0, self()]),
    ignore = negotiate(#{}, 12),
    accept = negotiate(#{client_max_window_bits_set => true}, 12),
    accept = negotiate(#{}, 15),
    %% A failed guard clears the exception before the next guard alternative.
    recovered = alternative(not_a_map),
    io:format("map_key_bif_ok~n"),
    ok.

check_maps([]) -> ok;
check_maps([{K, M, Expected}|Rest]) ->
    Expected = is_map_key(K, M),
    Expected = erlang:is_map_key(K, M),
    Expected = maps:is_key(K, M),
    Expected = apply(id(erlang), id(is_map_key), [K, M]),
    F = fun erlang:is_map_key/2,
    Expected = F(K, M),
    G = fun is_map_key/2,
    Expected = G(K, M),
    Expected = positive(K, M),
    Expected = qualified_positive(K, M),
    Absent = not Expected,
    Absent = negative(K, M),
    Absent = qualified_negative(K, M),
    Absent = not is_map_key(K, M),
    check_maps(Rest).

check_badmaps([]) -> ok;
check_badmaps([M|Rest]) ->
    {badmap, M} = reason(fun() -> is_map_key(a, M) end),
    {badmap, M} = reason(fun() -> erlang:is_map_key(a, M) end),
    {badmap, M} = reason(fun() -> maps:is_key(a, M) end),
    {badmap, M} = reason(fun() -> not is_map_key(a, M) end),
    {badmap, M} = reason(fun() -> apply(id(erlang), id(is_map_key), [a, M]) end),
    {badmap, M} = reason(fun() -> apply(id(maps), id(is_key), [a, M]) end),
    F = fun erlang:is_map_key/2,
    {badmap, M} = reason(fun() -> F(a, M) end),
    false = positive(a, M),
    false = qualified_positive(a, M),
    %% Non-maps fail the entire guard, not just its inner call.
    false = negative(a, M),
    false = qualified_negative(a, M),
    false = guard_orelse(a, M),
    check_badmaps(Rest).

positive(K, M) when is_map_key(K, M) -> true;
positive(_, _) -> false.
qualified_positive(K, M) when erlang:is_map_key(K, M) -> true;
qualified_positive(_, _) -> false.
negative(K, M) when not is_map_key(K, M) -> true;
negative(_, _) -> false.
qualified_negative(K, M) when not erlang:is_map_key(K, M) -> true;
qualified_negative(_, _) -> false.
guard_orelse(K, M) when is_map_key(K, M) orelse true -> true;
guard_orelse(_, _) -> false.
alternative(M) when is_map_key(a, M); M =:= not_a_map -> recovered;
alternative(_) -> failed.

%% The conjunction at Cowlib 2.21.0 cow_ws.erl:142.
negotiate(M, Bits) when not is_map_key(client_max_window_bits_set, M), Bits < 15 -> ignore;
negotiate(_, _) -> accept.

reason(F) -> try F() of V -> {unexpected, V} catch error:R -> R end.
id(X) -> X.
