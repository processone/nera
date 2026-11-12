%% EXPECT: generator_size_rebind_ok
-module(generator_size_rebind).
-export([test/0]).

%% NERA-52: a size reads the earlier generator's N; the segment after it
%% binds a new N. The entire binary must match, not just its sized prefix.
%% Run the same fixture natively and on BEAM.
test() ->
    L1 = id([<<1>>, <<2>>]),
    L2 = id([<<"p",1>>, <<"pq",2>>, <<"pq",1>>]),
    R = [{<<"p">>,1}, {<<"pq">>,2}, {<<"pq">>,1}],
    Cases = [
        {requested, R, requested(L1, L2)},
        {skip_bad_sizes, R, requested(id([<<0>>, <<1>>, <<2>>, <<4>>]),
            id([<<>>, <<"p",1>>, <<"pq",2>>, <<"pq",1>>, not_a_binary]))},
        {post_filter, [{<<"p">>,1}, {<<"pq">>,1}], filtered(L1, L2)},
        {third_generator, [{<<"p">>,1,7}, {<<"pq">>,2,7}, {<<"pq">>,1,7}], third(L1, L2)},
        {binary_body, <<"p",1,"pq",2,"pq",1>>, binary_body(L1, L2)},
        {constant_body, [matched,matched,matched], constant_body(L1, L2)},
        {expression_size, R, expression_size(L1, L2)},
        {prior_segment_wins, [{<<"p">>,1}, {<<"pq">>,2}, {<<"pq">>,2}],
            prior_segment(id([<<1,"p",1>>, <<2,"pq",2>>, <<2,"pq",1>>]))},
        {repeated_name, [{<<"p">>,1}, {<<"pq">>,2}],
            repeated(L1, id([<<"p",1,1>>, <<"pq",2,2>>, <<"pq",1,2>>]))},
        %% The missing end check belongs to the shared pattern compiler.
        %% Exercise each continuation: body, guard, sub-pattern, next pattern.
        {case_exact, {<<"p">>,1}, case_body(id(1), id(<<"p",1>>))},
        {case_extra, no, case_body(id(1), id(<<"pq",1>>))},
        {case_short, no, case_body(id(2), id(<<"p",1>>))},
        {case_zero, {<<>>,1}, case_body(id(0), id(<<1>>))},
        {case_zero_extra, no, case_body(id(0), id(<<1,2>>))},
        {case_partial_extra, no, case_body(id(1), id(<<"p",1,1:1>>))},
        {literal_exact, <<"p">>, case_literal(id(1), id(<<"p",1>>))},
        {literal_extra, no, case_literal(id(1), id(<<"p",1,2>>))},
        {guard_exact, <<"p">>, case_guard(id(1), id(<<"p",1>>))},
        {guard_extra, no, case_guard(id(1), id(<<"p",1,2>>))},
        {head_exact, {marker,<<"p">>,1}, head(id(<<1,"p",1>>), id(marker))},
        {head_extra, no, head(id(<<1,"p",1,2>>), id(marker))},
        {match_exact, {<<"p">>,1}, match_body(id(1), id(<<"p",1>>))},
        {match_extra, no, match_body(id(1), id(<<"p",1,2>>))},
        {maybe_exact, {<<"p">>,1}, maybe_body(id(1), id(<<"p",1>>))},
        {maybe_extra, no, maybe_body(id(1), id(<<"p",1,2>>))},
        {fun_exact, {<<"p">>,1}, fun_body(id(1), id(<<"p",1>>))},
        {fun_extra, no, fun_body(id(1), id(<<"p",1,2>>))},
        {receive_exact, {<<"p">>,1}, receive_body(id(1), id(<<"p",1>>))},
        {receive_extra, no, receive_body(id(1), id(<<"p",1,2>>))},
        %% Variable-width UTF takes the same minimum-size path.
        {utf8_exact, 16#20ac, utf8(id(<<16#20ac/utf8>>))},
        {utf8_extra, no, utf8(id(<<16#20ac/utf8,1>>))},
        {utf16_exact, 16#1f600, utf16(id(<<16#1f600/utf16>>))},
        {utf16_extra, no, utf16(id(<<16#1f600/utf16,1>>))},
        %% An explicit rest still accepts the remaining input.
        {rest, {<<"p">>,1,<<2>>}, rest(id(1), id(<<"p",1,2>>))}
    ],
    lists:foreach(fun({Name, Expected, Actual}) ->
        io:format("~p: ~p~n", [Name, Actual]),
        Expected = Actual
    end, Cases),
    io:format("generator_size_rebind_ok~n"),
    ok.

requested(L1,L2) -> [{B,N} || <<N:8>> <- L1, N > 0, <<B:N/binary,N:8>> <- L2].
filtered(L1,L2) -> [{B,N} || <<N:8>> <- L1, N > 0, <<B:N/binary,N:8>> <- L2, N =:= 1].
third(L1,L2) -> [{B,N,X} || <<N:8>> <- L1, N > 0, <<B:N/binary,N:8>> <- L2, X <- [7]].
binary_body(L1,L2) -> << <<B/binary,N>> || <<N:8>> <- L1, N > 0, <<B:N/binary,N:8>> <- L2 >>.
constant_body(L1,L2) -> [matched || <<N:8>> <- L1, N > 0, <<_B:N/binary,N:8>> <- L2].
expression_size(L1,L2) -> [{B,N} || <<N:8>> <- L1, N > 0, <<B:(N+0)/binary,N:8>> <- L2].
prior_segment(L) -> [{B,N} || <<N:8,B:N/binary,_:8>> <- L].
repeated(L1,L2) -> [{B,N} || <<N:8>> <- L1, N > 0, <<B:N/binary,N:8,N:8>> <- L2].

case_body(N, Bin) -> case Bin of <<B:N/binary,T:8>> -> {B,T}; _ -> no end.
case_literal(N, Bin) -> case Bin of <<B:N/binary,1:8>> -> B; _ -> no end.
case_guard(N, Bin) -> case Bin of <<B:N/binary,T:8>> when T =:= 1 -> B; _ -> no end.
head(<<N:8,B:N/binary,T:8>>, M) -> {M,B,T};
head(_, _) -> no.
match_body(N, Bin) ->
    try <<B:N/binary,T:8>> = Bin, {B,T}
    catch error:{badmatch,_} -> no end.
maybe_body(N, Bin) -> maybe <<B:N/binary,T:8>> ?= Bin, {B,T} else _ -> no end.
fun_body(N, Bin) ->
    F = fun(<<B:N/binary,T:8>>) -> {B,T}; (_) -> no end,
    F(Bin).
receive_body(N, Bin) ->
    self() ! Bin,
    receive <<B:N/binary,T:8>> -> {B,T}; _ -> no end.
utf8(Bin) -> case Bin of <<C/utf8>> -> C; _ -> no end.
utf16(Bin) -> case Bin of <<C/utf16>> -> C; _ -> no end.
rest(N, Bin) -> <<B:N/binary,T:8,R/binary>> = Bin, {B,T,R}.
id(X) -> X.
