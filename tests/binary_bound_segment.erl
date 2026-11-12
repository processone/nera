%% EXPECT: binary_bound_segment_ok
%% Regression: a bound variable used as a binary segment value constrains the
%% match by exact equality, and later fresh segments still bind. Cowlib 2.21.0
%% cow_multipart:parse_headers/2 matches
%% <<Boundary:BoundarySize/binary, "--", Stream2/bits>> with Boundary bound.
%% Every expected value below matches OTP 29.
-module(binary_bound_segment).
-export([test/0]).

test() ->
    lists:foreach(fun check/1, [
        %% cow_multipart shape: Stream is a deferred tail of the function head.
        {fun parse_headers/2, [<<"--abcd--epilogue">>, <<"abcd">>], {done, <<"epilogue">>}},
        {fun parse_headers/2, [<<"--abcd--">>, <<"abcd">>], {done, <<>>}},
        {fun parse_headers/2, [<<"--abcd--", 1:3>>, <<"abcd">>], {done, <<1:3>>}},
        {fun parse_headers/2, [<<"--abcd\r\nrest">>, <<"abcd">>], {part, <<"\r\nrest">>}},
        {fun parse_headers/2, [<<"--abce--x">>, <<"abcd">>], more},
        {fun parse_headers/2, [<<"--ab">>, <<"abcd">>], more},
        {fun parse_headers/2, [<<"----x">>, <<>>], {done, <<"x">>}},
        %% The brief's minimal module, with a fixed size.
        {fun fixed/2, [<<"abcd--x">>, <<"abcd">>], {done, <<"x">>}},
        {fun fixed/2, [<<"abce--x">>, <<"abcd">>], no},
        {fun fixed/2, [<<"abcd-+x">>, <<"abcd">>], no},
        {fun fixed/2, [<<"abcd--x">>, abcd], no},
        %% Integer, float and UTF segments compare with =:=.
        {fun int/2, [1, <<1, 2>>], {m, <<2>>}},
        {fun int/2, [2, <<1, 2>>], no},
        {fun int/2, [1.0, <<1, 2>>], no},
        {fun flt/2, [1.0, <<1.0/float, 7>>], {m, <<7>>}},
        {fun flt/2, [1, <<1.0/float, 7>>], no},
        {fun flt/2, [2.0, <<1.0/float, 7>>], no},
        {fun utf8/2, [233, <<233/utf8, $!>>], {m, <<"!">>}},
        {fun utf8/2, [$e, <<233/utf8, $!>>], no},
        {fun utf16/2, [16#1F600, <<16#1F600/utf16>>], {m, <<>>}},
        {fun utf16/2, [16#1F601, <<16#1F600/utf16>>], no},
        {fun utf32/2, [$z, <<$z/utf32-little>>], {m, <<>>}},
        {fun utf32/2, [$y, <<$z/utf32-little>>], no},
        %% A bound tail compares the whole remaining bitstring.
        {fun tail/2, [<<2, 3>>, <<1, 2, 3>>], m},
        {fun tail/2, [<<2>>, <<1, 2, 3>>], no},
        {fun tail/2, [<<2, 3, 4>>, <<1, 2, 3>>], no},
        %% The bound value itself is unchanged by a matching attempt.
        {fun keep/2, [2, <<1>>], {no, 2}},
        %% A variable repeated inside one binary, or across positions.
        {fun repeated/1, [<<5, 5, 6>>], {m, 5, <<6>>}},
        {fun repeated/1, [<<5, 4, 6>>], no},
        {fun size_and_value/1, [<<3, 3:3>>], {m, 3}},
        {fun size_and_value/1, [<<3, 2:3>>], no},
        {fun repeated_tail/1, [<<"abab">>], {m, <<"ab">>}},
        {fun repeated_tail/1, [<<"abac">>], no},
        {fun tuple_after/1, [{<<1>>, 1}], {m, 1}},
        {fun tuple_after/1, [{<<1>>, 2}], no},
        {fun tuple_before/1, [{1, <<1>>}], {m, 1}},
        {fun tuple_before/1, [{2, <<1>>}], no},
        {fun two_binaries/1, [{<<4>>, <<4>>}], {m, 4}},
        {fun two_binaries/1, [{<<4>>, <<5>>}], no},
        %% A deferred tail bound earlier can be the bound value.
        {fun deferred/2, [<<0, "xy">>, <<"xyz">>], {m, <<"z">>}},
        {fun deferred/2, [<<0, "xy">>, <<"xqz">>], no},
        %% Recursive heads read the bound value from a reused match context.
        {fun count_until/3, [<<"cd">>, <<"abcd">>, 0], 2},
        {fun count_until/3, [<<"zz">>, <<"abcd">>, 0], notfound},
        {fun count_until/3, [<<>>, <<"ab">>, 0], 2},
        {fun find/3, [<<"cd">>, <<"abcde">>, 0], 2},
        {fun find/3, [<<"zz">>, <<"abcde">>, 0], notfound},
        %% Function heads, match expressions, guards, receive and maybe.
        {fun head/2, [1, <<1, 9>>], {m, <<9>>}},
        {fun head/2, [2, <<1, 9>>], no},
        {fun match_expr/2, [1, <<1, 9>>], {m, <<9>>}},
        {fun match_expr/2, [2, <<1, 9>>], {error, {badmatch, <<1, 9>>}}},
        {fun repeated_match/1, [<<5, 5, 6>>], {5, <<6>>}},
        {fun repeated_match/1, [<<5, 4, 6>>], {error, {badmatch, <<5, 4, 6>>}}},
        {fun guarded/2, [1, <<1, "ab">>], {m, <<"ab">>}},
        {fun guarded/2, [1, <<1, "a">>], no},
        {fun received/1, [7], {m, <<"tail">>}},
        {fun maybe_expr/2, [1, <<1, 9>>], {m, <<9>>}},
        {fun maybe_expr/2, [1, <<2, 9>>], {fallback, <<2, 9>>}},
        %% maybe ?= and comprehension generators bind the names before the
        %% match runs; a repeated or bound name must still compare.
        {fun maybe_repeated/1, [<<"xx">>], {m, <<"x">>}},
        {fun maybe_repeated/1, [<<"xy">>], {fallback, <<"xy">>}},
        {fun maybe_pair/1, [{<<"x">>, <<"x">>}], {m, <<"x">>}},
        {fun maybe_pair/1, [{<<"x">>, <<"y">>}], no},
        {fun maybe_bound_tail/1, [<<"xx">>], {m, <<"x">>}},
        {fun maybe_bound_tail/1, [<<"xy">>], no},
        {fun maybe_utf/1, [<<233/utf8, 233/utf8>>], {m, 233}},
        {fun maybe_utf/1, [<<233/utf8, $e>>], no},
        {fun maybe_chain/2, [<<0, "ab">>, <<"abc">>], {<<"ab">>, <<"c">>}},
        {fun maybe_chain/2, [<<0, "ab">>, <<"xbc">>], no},
        {fun lc_repeated/1, [[<<"xx">>, <<"xy">>, <<"zz">>]], [<<"x">>, <<"z">>]},
        {fun lc_repeated_unused/1, [[<<"xx">>, <<"xy">>, <<"zz">>]], [m, m]},
        {fun lc_pair_unused/1, [[{<<"x">>, <<"x">>}, {<<"x">>, <<"y">>}]], [m]},
        {fun lc_tail_unused/1, [[<<"xx">>, <<"xy">>, <<"zzz">>]], [m]},
        {fun lc_filter/1, [[<<"xx">>, <<"xy">>, <<"zz">>]], [m]},
        {fun bc_repeated/1, [[<<"xx">>, <<"xy">>, <<"zz">>]], <<"xz">>},
        {fun bc_repeated_unused/1, [[<<"xx">>, <<"xy">>, <<"zz">>]], <<1, 1>>},
        {fun bc_tail_unused/1, [[<<"xx">>, <<"xy">>, <<"zzz">>]], <<1>>},
        %% try-of and catch leave an outer deferred tail intact, whatever
        %% value reaches their binary pattern.
        {fun try_of_keeps_tail/2, [<<0, "a">>, <<"b">>], <<"a">>},
        {fun try_of_keeps_tail/2, [<<0, "a">>, <<"a">>], <<"a">>},
        {fun try_of_keeps_tail/2, [<<0, "a">>, <<"bc">>], <<"a">>},
        {fun try_of_keeps_tail/2, [<<0, "a">>, nope], <<"a">>},
        {fun catch_keeps_tail/2, [<<0, "a">>, <<"b">>], <<"a">>},
        {fun catch_keeps_tail/2, [<<0, "a">>, nope], <<"a">>},
        {fun catch_class_var_keeps_tail/2, [<<0, "a">>, <<"b">>], <<"a">>},
        {fun catch_class_var_keeps_tail/2, [<<0, "a">>, nope], <<"a">>},
        %% Fun heads and generators introduce fresh variables instead,
        %% including a name an earlier generator bound.
        {fun shadowing/1, [9], {9, [{1, <<2>>}, {3, <<4>>}]}},
        {fun lc_shadow/2, [[<<"a">>, <<"b">>], [<<"pa">>, <<"qb">>]],
         [{<<"a">>, <<"p">>}, {<<"b">>, <<"q">>}, {<<"a">>, <<"p">>}, {<<"b">>, <<"q">>}]},
        {fun bc_shadow/2, [[<<"a">>, <<"b">>], [<<"pa">>, <<"qb">>]], <<"apbqapbq">>},
        {fun lc_shadow_repeated/2, [[<<"a">>], [<<"xx">>, <<"xy">>]], [<<"x">>]},
        {fun lc_outer_size/2, [[<<1>>, <<2>>], [<<"pq">>, <<"r">>]], [<<"p">>, <<"r">>, <<"pq">>]}
    ]),
    io:format("binary_bound_segment_ok~n"),
    ok.

%% Arguments come from a list, so the compiler cannot fold any match away.
check({Fun, Args, Expected}) ->
    case result(Fun, Args) of
        Expected -> ok;
        Actual -> erlang:error({mismatch, Fun, Args, Expected, Actual})
    end.

result(Fun, Args) ->
    try apply(Fun, Args) catch error:Reason -> {error, Reason} end.

%% Same structure as cow_multipart:parse_headers/2 (Cowlib 2.21.0).
parse_headers(<<"--", Stream/bits>>, Boundary) ->
    BoundarySize = byte_size(Boundary),
    case Stream of
        <<Boundary:BoundarySize/binary, "--", Stream2/bits>> ->
            {done, Stream2};
        <<Boundary:BoundarySize/binary, Stream2/bits>> ->
            {part, Stream2};
        _ ->
            more
    end;
parse_headers(_, _) ->
    more.

fixed(Stream, Boundary) ->
    case Stream of
        <<Boundary:4/binary, "--", Stream2/bits>> -> {done, Stream2};
        _ -> no
    end.

int(X, Bin) -> case Bin of <<X:8, R/binary>> -> {m, R}; _ -> no end.
flt(X, Bin) -> case Bin of <<X/float, R/binary>> -> {m, R}; _ -> no end.
utf8(X, Bin) -> case Bin of <<X/utf8, R/binary>> -> {m, R}; _ -> no end.
utf16(X, Bin) -> case Bin of <<X/utf16, R/binary>> -> {m, R}; _ -> no end.
utf32(X, Bin) -> case Bin of <<X/utf32-little, R/binary>> -> {m, R}; _ -> no end.
tail(R, Bin) -> case Bin of <<_:8, R/binary>> -> m; _ -> no end.
keep(X, Bin) -> {case Bin of <<X:8>> -> m; _ -> no end, X}.

repeated(Bin) -> case Bin of <<A:8, A:8, R/binary>> -> {m, A, R}; _ -> no end.
size_and_value(Bin) -> case Bin of <<N:8, N:N>> -> {m, N}; _ -> no end.
repeated_tail(Bin) -> case Bin of <<A:2/binary, A/binary>> -> {m, A}; _ -> no end.
tuple_after(T) -> case T of {<<A:8>>, A} -> {m, A}; _ -> no end.
tuple_before(T) -> case T of {A, <<A:8>>} -> {m, A}; _ -> no end.
two_binaries(T) -> case T of {<<A:8>>, <<A:8>>} -> {m, A}; _ -> no end.

deferred(<<0, Prefix/binary>>, Bin) ->
    case Bin of <<Prefix:2/binary, R/binary>> -> {m, R}; _ -> no end.

count_until(Suffix, <<Suffix/binary>>, N) -> N;
count_until(Suffix, <<_:8, Rest/binary>>, N) -> count_until(Suffix, Rest, N + 1);
count_until(_, <<>>, _) -> notfound.

find(Pat, <<Pat:2/binary, _/binary>>, N) -> N;
find(Pat, <<_:8, Rest/binary>>, N) -> find(Pat, Rest, N + 1);
find(_, <<>>, _) -> notfound.

head(X, <<X:8, R/binary>>) -> {m, R};
head(_, _) -> no.

match_expr(X, Bin) ->
    <<X:8, R/binary>> = Bin,
    {m, R}.

repeated_match(Bin) ->
    <<A:8, A:8, R/binary>> = Bin,
    {A, R}.

guarded(X, Bin) ->
    case Bin of
        <<X:8, R/binary>> when byte_size(R) =:= 2 -> {m, R};
        _ -> no
    end.

received(Tag) ->
    self() ! <<8, "skip">>,
    self() ! <<Tag, "tail">>,
    Result = receive <<Tag:8, R/binary>> -> {m, R} end,
    receive <<8, "skip">> -> Result end.

maybe_expr(X, Bin) ->
    maybe
        <<X:8, R/binary>> ?= Bin,
        {m, R}
    else
        Other -> {fallback, Other}
    end.

maybe_repeated(Bin) ->
    maybe <<A:1/binary, A:1/binary>> ?= Bin, {m, A} else Other -> {fallback, Other} end.
maybe_pair(T) -> maybe {<<A:1/binary>>, <<A:1/binary>>} ?= T, {m, A} else _ -> no end.
maybe_bound_tail(Bin) -> maybe <<A:1/binary, A/binary>> ?= Bin, {m, A} else _ -> no end.
maybe_utf(Bin) -> maybe <<A/utf8, A/utf8>> ?= Bin, {m, A} else _ -> no end.
maybe_chain(X, Y) ->
    maybe
        <<_:8, A/binary>> ?= X,
        <<A:2/binary, R/binary>> ?= Y,
        {A, R}
    else
        _ -> no
    end.

lc_repeated(L) -> [A || <<A:1/binary, A:1/binary>> <- L].
lc_repeated_unused(L) -> [m || <<A:1/binary, A:1/binary>> <- L].
lc_pair_unused(L) -> [m || {<<A:1/binary>>, <<A:1/binary>>} <- L].
lc_tail_unused(L) -> [m || <<A:1/binary, A/binary>> <- L].
lc_filter(L) -> [m || <<A:1/binary, A:1/binary>> <- L, A =/= <<"z">>].
bc_repeated(L) -> << <<A/binary>> || <<A:1/binary, A:1/binary>> <- L >>.
bc_repeated_unused(L) -> << <<1>> || <<A:1/binary, A:1/binary>> <- L >>.
bc_tail_unused(L) -> << <<1>> || <<A:1/binary, A/binary>> <- L >>.

%% X is a live deferred tail of the head. Every clause returns ok, so only X's
%% value after the try is observed.
try_of_keeps_tail(<<0, X/binary>>, Y) ->
    try Y of <<X:1/binary>> -> ok; _ -> ok catch _:_ -> ok end,
    X.
catch_keeps_tail(<<0, X/binary>>, Y) ->
    try throw(Y) catch throw:<<X:1/binary>> -> ok; _:_ -> ok end,
    X.
catch_class_var_keeps_tail(<<0, X/binary>>, Y) ->
    try throw(Y) catch Class:<<X:1/binary>> when Class =:= throw -> ok; _:_ -> ok end,
    X.

%% Each generator's A shadows the earlier generator's A, as in OTP.
lc_shadow(L1, L2) ->
    [{A, B} || <<A:1/binary>> <- L1, A =/= <<>>, <<B:1/binary, A:1/binary>> <- L2].
bc_shadow(L1, L2) ->
    << <<A/binary, B/binary>> || <<A:1/binary>> <- L1, A =/= <<>>,
                                 <<B:1/binary, A:1/binary>> <- L2 >>.
lc_shadow_repeated(L1, L2) ->
    [A || <<A:1/binary>> <- L1, A =/= <<>>, <<A:1/binary, A:1/binary>> <- L2].
lc_outer_size(L1, L2) -> [B || <<N:8>> <- L1, <<B:N/binary, _/binary>> <- L2].

%% X is bound here, but fun heads and generator patterns shadow it, as in OTP.
shadowing(X) ->
    F = fun(<<X:8, R/binary>>) -> {X, R} end,
    Expected = [F(<<1, 2>>), F(<<3, 4>>)],
    Expected = [{X, R} || <<X:8, R/binary>> <- [<<1, 2>>, <<3, 4>>]],
    {X, Expected}.
