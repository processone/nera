%% EXPECT: pattern_input_scope_ok
-module(pattern_input_scope).
-export([test/0]).

%% NERA-54: key/size reads use the inputs to a fun head or generator,
%% while value bindings shadow them. Run this same regression on BEAM.
test() ->
    v = fun_key(id(k), id(#{k => v})),
    no = fun_key(id(k), id(#{q => w})),
    v = fun_expression_key(id(k), id(#{{route, k} => v})),
    1286 = fun_size(id(16), id(<<5, 6>>)),
    1286 = fun_size_expression(id(8), id(<<5, 6>>)),
    <<5, 6>> = fun_binary_size(id(2), id(<<5, 6, 7>>)),
    no = fun_size(id(24), id(<<5, 6>>)),
    {q, v} = fun_shadow_key(id(k), id(#{a => q, k => v})),
    no = fun_shadow_key(id(k), id(#{a => q, q => w})),
    {q, v} = fun_argument_key(id(k), id(q), id(#{k => v})),
    {16, 5} = fun_argument_size(id(8), id(16), id(<<5, 6>>)),
    {16, 5} = fun_sibling_size(id(8), id(#{a => 16, b => <<5, 6>>})),
    {16, 5} = fun_segment_size(id(8), id(<<16, 0, 5, 6>>)),
    {16, 5} = fun_separate_binaries(id(8), id(<<16>>), id(<<5>>)),
    v = named_fun_key(id(k), id([#{k => v}])),
    v = nested_fun_key(id(k), id(#{k => v})),
    v = fun_case_key(id(k), id(#{k => v})),
    v = fun_clause_key(id(k), id(#{k => v})),
    {above, below} = fun_guard(id(8), id(9)),
    [v] = list_key(id(k), id([#{k => v}, #{q => w}, not_a_map])),
    [{q, v}] = list_shadow_key(id(k), id([#{a => q, k => v}, #{a => q, q => w}])),
    [v, w] = list_earlier_key(id([{k, 1}, {q, 2}]), id([#{k => v, q => w}])),
    [{q, v}, {r, w}] = list_earlier_shadow_key(id([{k, 1}, {q, 2}]), id([#{a => q, k => v}, #{a => r, q => w}])),
    [5, 1286] = list_earlier_size(id([{8, x}, {16, y}]), id([<<5, 6>>])),
    [1286] = list_size(id(16), id([<<5, 6>>, <<5>>])),
    [{16, 5}] = list_shadow_size(id(8), id([{16, <<5, 6>>}])),
    [{16, 5}] = list_segment_size(id(8), id([<<16, 0, 5, 6>>])),
    [v] = list_third_key(id([#{a => k}]), id([#{k => v}])),
    [v] = nested_list_key(id(k), id([#{k => v}])),
    <<5>> = binary_key(id(k), id([#{k => 5}, #{q => 6}])),
    <<5>> = binary_shadow_key(id(k), id([#{a => q, k => 5}, #{a => q, q => 6}])),
    <<5, 6>> = binary_earlier_key(id([{k, 1}, {q, 2}]), id([#{k => 5, q => 6}])),
    <<5>> = binary_size(id(16), id([<<0, 5>>])),
    <<5, 6>> = binary_earlier_size(id([{8, x}, {16, y}]), id([<<5, 6>>])),
    v = tail_fun_key(id(<<0, "k">>), id(#{<<"k">> => v})),
    [v] = tail_list_key(id(<<0, "k">>), id([#{<<"k">> => v}])),
    [v, w] = tail_earlier_key(id([<<0, "k">>, <<0, "q">>]), id([#{<<"k">> => v, <<"q">> => w}])),
    [{<<"k">>, v}, {<<"q">>, w}] = tail_earlier_read(id([<<0, "k">>, <<0, "q">>]), id([#{<<"k">> => v, <<"q">> => w}])),
    [v, w] = tail_earlier_filter(id([<<0, "k">>, <<0, "q">>]), id([#{<<"k">> => v, <<"q">> => w}])),
    <<"k", 5, "q", 6>> = binary_tail_read(id([<<0, "k">>, <<0, "q">>]), id([#{<<"k">> => 5, <<"q">> => 6}])),
    %% NERA-80: rejecting same-head inputs must keep sequential binary sizes.
    F = fun(<<N, X:N>>) -> X end,
    5 = F(id(<<8, 5>>)),
    [5] = [X || <<N, X:N>> <- id([<<8, 5>>])],
    [5] = [X || <<N, X:N>> <= id(<<8, 5>>)],
    <<5>> = << <<X>> || <<N, X:N>> <= id(<<8, 5>>) >>,
    <<5>> = << <<X>> || <<N>> <= id(<<8>>), <<X:N>> <= id(<<5>>) >>,
    io:format("pattern_input_scope_ok~n"),
    ok.

fun_key(K, M) ->
    F = fun(#{K := V}) -> V; (_) -> no end,
    F(M).

fun_size(N, B) ->
    F = fun(<<X:N, _/binary>>) -> X; (_) -> no end,
    F(B).

fun_expression_key(K, M) ->
    F = fun(#{{route, K} := V}) -> V end,
    F(M).

fun_size_expression(N, B) ->
    F = fun(<<X:(N + 8), _/binary>>) -> X end,
    F(B).

fun_binary_size(N, B) ->
    F = fun(<<X:N/binary, _/binary>>) -> X end,
    F(B).

fun_shadow_key(K, M) ->
    F = fun(#{a := K, K := V}) -> {K, V}; (_) -> no end,
    F(M).

fun_argument_key(K, A, M) ->
    F = fun(K, #{K := V}) -> {K, V} end,
    F(A, M).

fun_argument_size(N, A, B) ->
    F = fun(N, <<X:N, _/binary>>) -> {N, X} end,
    F(A, B).

fun_sibling_size(N, M) ->
    F = fun(#{a := N, b := <<X:N, _/binary>>}) -> {N, X} end,
    F(M).

fun_segment_size(_N, B) ->
    %% A size within this binary sees its earlier segment, not the outer N.
    F = fun(<<N:8, X:N, _/binary>>) -> {N, X} end,
    F(B).

fun_separate_binaries(N, A, B) ->
    F = fun(<<N:8>>, <<X:N>>) -> {N, X} end,
    F(A, B).

named_fun_key(K, M) ->
    F = fun Again(#{K := V}) -> V; Again([H | _]) -> Again(H) end,
    F(M).

nested_fun_key(K, M) ->
    Make = fun() -> fun(#{K := V}) -> V end end,
    F = Make(),
    F(M).

fun_case_key(K, M) ->
    F = fun(Map) -> case Map of #{K := V} -> V end end,
    F(M).

fun_clause_key(K, M) ->
    F = fun(#{K := V}) -> V; (K) -> K end,
    F(M).

%% NERA-77 I-3: captures are available before the guard chooses a clause.
fun_guard(Limit, Value) ->
    F = fun(X) when X > Limit -> above; (_) -> below end,
    {F(Value), F(Limit)}.

list_key(K, Maps) -> [V || #{K := V} <- Maps].

list_shadow_key(K, Maps) -> [{K, V} || #{a := K, K := V} <- Maps].

list_earlier_key(Keys, Maps) -> [V || {K, _} <- Keys, #{K := V} <- Maps].

list_earlier_shadow_key(Keys, Maps) ->
    [{K, V} || {K, _} <- Keys, #{a := K, K := V} <- Maps].

list_earlier_size(Sizes, Bins) -> [X || {N, _} <- Sizes, <<X:N, _/binary>> <- Bins].

list_size(N, Bins) -> [X || <<X:N, _/binary>> <- Bins].

list_shadow_size(N, Pairs) -> [{N, X} || {N, <<X:N, _/binary>>} <- Pairs].

list_segment_size(_N, Bins) -> [{N, X} || <<N:8, X:N, _/binary>> <- Bins].

list_third_key(Keys, Maps) -> [V || #{a := K} <- Keys, _ <- [ok], #{K := V} <- Maps].

nested_list_key(K, Maps) ->
    F = fun() -> [V || #{K := V} <- Maps] end,
    F().

binary_key(K, Maps) -> << <<V>> || #{K := V} <- Maps >>.

binary_shadow_key(K, Maps) -> << <<V>> || #{a := K, K := V} <- Maps >>.

binary_earlier_key(Keys, Maps) -> << <<V>> || {K, _} <- Keys, #{K := V} <- Maps >>.

binary_size(N, Bins) -> << <<X>> || <<X:N, _/binary>> <- Bins >>.

binary_earlier_size(Sizes, Bins) -> << <<X>> || {N, _} <- Sizes, <<X:N, _/binary>> <- Bins >>.

tail_fun_key(<<_, K/binary>>, M) ->
    F = fun(#{K := V}) -> V end,
    F(M).

tail_list_key(<<_, K/binary>>, Maps) -> [V || #{K := V} <- Maps].

tail_earlier_key(Bins, Maps) -> [V || <<_, K/binary>> <- Bins, #{K := V} <- Maps].

tail_earlier_read(Bins, Maps) -> [{K, V} || <<_, K/binary>> <- Bins, #{K := V} <- Maps].

tail_earlier_filter(Bins, Maps) -> [V || <<_, K/binary>> <- Bins, #{K := V} <- Maps, byte_size(K) =:= 1].

binary_tail_read(Bins, Maps) -> << <<K/binary, V>> || <<_, K/binary>> <- Bins, #{K := V} <- Maps >>.

id(X) -> X.
