%% EXPECT: binary_rest_unit_ok
%% NERA-49: an unsized /binary segment accepts only a remainder divisible by
%% its effective unit (8 bits by default), as in BEAM. /bits stays bit-sized.
-module(binary_rest_unit).
-export([test/0]).

test() ->
    lists:foreach(fun check/1, [
        %% The reported failure, with positive and empty-tail controls.
        {fun prefix_case/1, [<<1,1:2>>], no},
        {fun prefix_case/1, [<<1,2>>], {m,<<2>>}},
        {fun prefix_case/1, [<<1>>], {m,<<>>}},
        {fun prefix_case/1, [<<1:2>>], no},
        {fun whole_case/1, [<<1:2>>], no},
        {fun whole_case/1, [<<1,2>>], {m,<<1,2>>}},
        {fun whole_case/1, [<<>>], {m,<<>>}},
        {fun whole_case/1, [not_binary], no},
        {fun prefix_head/1, [<<1,1:2>>], no},
        {fun prefix_head/1, [<<1,2>>], {m,<<2>>}},
        {fun prefix_head/1, [<<1>>], {m,<<>>}},
        {fun whole_head/1, [<<1:2>>], no},
        {fun whole_head/1, [<<2>>], {m,<<2>>}},
        {fun whole_head/1, [<<>>], {m,<<>>}},
        %% A match expression must raise badmatch, not bind a bitstring.
        {fun prefix_match/1, [<<1,1:2>>], {error,{badmatch,<<1,1:2>>}}},
        {fun prefix_match/1, [<<1,2>>], {m,<<2>>}},
        {fun prefix_match/1, [<<1>>], {m,<<>>}},
        {fun whole_match/1, [<<1:2>>], {error,{badmatch,<<1:2>>}}},
        {fun whole_match/1, [<<2>>], {m,<<2>>}},
        {fun whole_match/1, [<<>>], {m,<<>>}},
        %% Failure must reach a /bits clause, even for an unused tail.
        {fun fallback/1, [<<1,1:2>>], {bits,<<1:2>>}},
        {fun fallback/1, [<<1,2>>], {binary,<<2>>}},
        {fun fallback/1, [<<1>>], {binary,<<>>}},
        {fun wildcard/1, [<<1,1:2>>], bits},
        {fun wildcard/1, [<<1,2>>], binary},
        {fun wildcard/1, [<<1>>], binary},
        %% Check the remainder size, not the whole input or physical offset.
        {fun shifted/1, [<<5:3,42>>], {m,<<42>>}},
        {fun shifted/1, [<<5:3,1:2>>], no},
        {fun shifted/1, [<<5:3>>], {m,<<>>}},
        {fun dynamic/2, [3,<<5:3,42>>], {m,<<42>>}},
        {fun dynamic/2, [3,<<5:3,1:2>>], no},
        {fun dynamic/2, [3,<<5:3>>], {m,<<>>}},
        {fun dynamic/2, [9,<<1>>], no},
        %% A deferred tail can reach another function as a match context.
        {fun reuse/1, [<<5:3,42>>], {m,<<42>>}},
        {fun reuse/1, [<<5:3,1:2>>], no},
        {fun recurse/1, [<<0:2,42>>], {m,<<42>>}},
        {fun recurse/1, [<<0:1,42>>], {m,<<42>>}},
        %% Bound and repeated tails must obey the same unit constraint.
        {fun bound/2, [<<1:2>>,<<0,1:2>>], no},
        {fun bound/2, [<<2>>,<<0,2>>], m},
        {fun bound/2, [<<3>>,<<0,2>>], no},
        {fun deferred_bound/2, [<<0,1:2>>,<<0,1:2>>], no},
        {fun deferred_bound/2, [<<0,2>>,<<0,2>>], m},
        {fun repeated/1, [<<1:2,1:2>>], no},
        %% Explicit units constrain divisibility, not necessarily bytes.
        {fun unit16/1, [<<1>>], no},
        {fun unit16/1, [<<1,2>>], {m,<<1,2>>}},
        {fun unit16/1, [<<>>], {m,<<>>}},
        {fun unit3/1, [<<1:3>>], {m,<<1:3>>}},
        {fun unit3/1, [<<1:4>>], no},
        {fun unit3/1, [<<1:6>>], {m,<<1:6>>}},
        {fun unit1/1, [<<1:2>>], {m,<<1:2>>}},
        {fun bitstring/1, [<<1:2>>], {m,<<1:2>>}},
        %% The new branch must not bypass UTF, literals or guards.
        {fun utf/1, [<<233/utf8,1:2>>], no},
        {fun utf/1, [<<233/utf8,42>>], {m,<<42>>}},
        {fun literal_guard/1, [<<7,42,1:2>>], no},
        {fun literal_guard/1, [<<7,42>>], {m,<<42>>}},
        {fun literal_guard/1, [<<8,42>>], no},
        {fun literal_guard/1, [<<7,42,43>>], no},
        %% Other callers of the shared binary pattern compiler.
        {fun maybe_match/1, [<<1,1:2>>], no},
        {fun maybe_match/1, [<<1,2>>], {m,<<2>>}},
        {fun received/1, [<<1,1:2>>], {bits,<<1:2>>}},
        {fun received/1, [<<1,2>>], {binary,<<2>>}},
        {fun list_generator/1, [[<<1,1:2>>,<<1,2>>,<<1>>]], [<<2>>,<<>>]}
    ]),
    io:format("binary_rest_unit_ok~n"),
    ok.

%% Table inputs prevent folding the matches; check both the value and errors.
check({Fun, Args, Expected}) ->
    Actual = try apply(Fun, Args) catch error:Reason -> {error,Reason} end,
    case Actual of
        Expected -> ok;
        _ -> erlang:error({mismatch,Fun,Args,Expected,Actual})
    end.

prefix_case(B) -> case B of <<_:8,R/binary>> -> {m,R}; _ -> no end.
whole_case(B) -> case B of <<R/binary>> -> {m,R}; _ -> no end.
prefix_head(<<_:8,R/binary>>) -> {m,R}; prefix_head(_) -> no.
whole_head(<<R/binary>>) -> {m,R}; whole_head(_) -> no.
prefix_match(B) -> <<_:8,R/binary>> = B, {m,R}.
whole_match(B) -> <<R/binary>> = B, {m,R}.
fallback(B) ->
    case B of
        <<_:8,R/binary>> -> {binary,R};
        <<_:8,R/bits>> -> {bits,R}
    end.
wildcard(B) ->
    case B of
        <<_:8,_/binary>> -> binary;
        <<_:8,_/bits>> -> bits
    end.
shifted(B) -> case B of <<_:3,R/binary>> -> {m,R}; _ -> no end.
dynamic(N,B) -> case B of <<_:N,R/binary>> -> {m,R}; _ -> no end.
reuse(<<_:3,R/bits>>) -> whole_head(R).
recurse(<<_:1,R/binary>>) -> {m,R};
recurse(<<_:1,R/bits>>) -> recurse(R).
bound(R,B) -> case B of <<_:8,R/binary>> -> m; _ -> no end.
deferred_bound(<<_:8,R/bits>>,B) -> bound(R,B).
repeated(B) -> case B of <<R:2/bits,R/binary>> -> m; _ -> no end.
unit16(B) -> case B of <<R/binary-unit:16>> -> {m,R}; _ -> no end.
unit3(B) -> case B of <<R/binary-unit:3>> -> {m,R}; _ -> no end.
unit1(B) -> case B of <<R/binary-unit:1>> -> {m,R}; _ -> no end.
bitstring(B) -> case B of <<R/bitstring>> -> {m,R}; _ -> no end.
utf(B) -> case B of <<_/utf8,R/binary>> -> {m,R}; _ -> no end.
literal_guard(B) ->
    case B of <<7,R/binary>> when byte_size(R) =:= 1 -> {m,R}; _ -> no end.
maybe_match(B) -> maybe <<_:8,R/binary>> ?= B, {m,R} else _ -> no end.
received(B) ->
    self() ! B,
    receive <<_:8,R/binary>> -> {binary,R}; <<_:8,R/bits>> -> {bits,R} end.
list_generator(L) -> [R || <<_:8,R/binary>> <- L].
