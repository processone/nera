%% EXPECT: binary_tail_byte_size_ok
%% NERA-76: byte_size/1 rounds deferred partial-byte tails up, as on BEAM.
-module(binary_tail_byte_size).
-export([test/0]).

test() ->
    %% Explicit byte/bit counts, including the reported <<1,1:2>> input.
    lists:foreach(fun({Tail, Bytes, Bits}) ->
        B = <<1,Tail/bits>>,
        lists:foreach(fun({Name, Fun}) ->
            check(Name, B, {Bytes,Bits}, Fun(B))
        end, [
            {head, fun head/1}, {bitstring, fun bitstring/1},
            {unit1, fun unit1/1}, {case_body, fun case_body/1},
            {match_body, fun match_body/1}, {maybe_body, fun maybe_body/1},
            {receive_body, fun receive_body/1}, {generator, fun generator/1}
        ]),
        check(guard, B, {Bytes,Bits}, guarded(B, Bytes)),
        no = guarded(B, Bytes + 1),
        check(case_guard, B, {Bytes,Bits}, case_guard(B, Bytes)),
        no = case_guard(B, Bytes + 1),
        %% Exercise offsets that are not byte-aligned and a reused cursor.
        Shifted = <<5:3,Tail/bits>>,
        check(shifted, Shifted, {Bytes,Bits}, shifted(Shifted)),
        check(dynamic, Shifted, {Bytes,Bits}, dynamic(3, Shifted)),
        check(recursive, Shifted, {Bytes,Bits}, recursive(Shifted, 3)),
        %% A qualified call materializes the same tail; later calls must agree.
        check(materialized, B, {Bytes,Bytes,Bytes,Bits}, materialized(B))
    end, [
        {<<>>,0,0}, {<<1:1>>,1,1}, {<<1:2>>,1,2}, {<<5:3>>,1,3},
        {<<9:4>>,1,4}, {<<17:5>>,1,5}, {<<33:6>>,1,6}, {<<65:7>>,1,7},
        {<<165>>,1,8}, {<<165,1:1>>,2,9}, {<<165,1:2>>,2,10},
        {<<165,5:3>>,2,11}, {<<165,9:4>>,2,12}, {<<165,17:5>>,2,13},
        {<<165,33:6>>,2,14}, {<<165,65:7>>,2,15}, {<<165,90>>,2,16},
        {<<165,90,1:1>>,3,17}, {<<0:520,1:2>>,66,522}
    ]),
    %% Non-byte units still enforce divisibility while rounding byte_size up.
    lists:foreach(fun({Tail, Bytes, Bits}) ->
        B = <<1,Tail/bits>>,
        check(unit3, B, {Bytes,Bits}, unit3(B)),
        check(unit3_guard, B, {Bytes,Bits}, unit3_guard(B, Bytes)),
        no = unit3_guard(B, Bytes + 1)
    end, [
        {<<>>,0,0}, {<<5:3>>,1,3}, {<<33:6>>,1,6}, {<<257:9>>,2,9},
        {<<2049:12>>,2,12}, {<<16385:15>>,2,15}, {<<165,90,42>>,3,24},
        {<<165,90,42,5:3>>,4,27}
    ]),
    %% NERA-84 F2: case bodies also defer sized bitstring segments.
    Sized = id(<<5:3,1:2>>),
    check(sized_case, Sized, {1,3}, sized_case(Sized)),
    {2,12} = unit12(id(<<1,2049:12>>)),
    {3,24} = unit12(id(<<1,165,90,42>>)),
    {0,0} = binary(id(<<1>>)),
    {1,8} = binary(id(<<1,42>>)),
    {2,16} = binary(id(<<1,42,43>>)),
    {2,16} = unit16(id(<<1,42,43>>)),
    io:format("binary_tail_byte_size_ok~n"),
    ok.

check(_, _, Expected, Expected) -> ok;
check(Name, B, Expected, Actual) ->
    erlang:error({tail_size_mismatch,Name,B,Expected,Actual}).

head(<<_:8,R/bits>>) -> {byte_size(R),bit_size(R)}.
bitstring(<<_:8,R/bitstring>>) -> {byte_size(R),bit_size(R)}.
unit1(<<_:8,R/binary-unit:1>>) -> {byte_size(R),bit_size(R)}.
unit3(<<_:8,R/binary-unit:3>>) -> {byte_size(R),bit_size(R)}.
unit12(<<_:8,R/binary-unit:12>>) -> {byte_size(R),bit_size(R)}.
unit16(<<_:8,R/binary-unit:16>>) -> {byte_size(R),bit_size(R)}.
binary(<<_:8,R/binary>>) -> {byte_size(R),bit_size(R)}.
case_body(B) -> case B of <<_:8,R/bits>> -> {byte_size(R),bit_size(R)} end.
sized_case(B) -> case B of <<X:3/bits,_/bits>> -> {byte_size(X),bit_size(X)} end.
match_body(B) -> <<_:8,R/bits>> = B, {byte_size(R),bit_size(R)}.
maybe_body(B) -> maybe <<_:8,R/bits>> ?= B, {byte_size(R),bit_size(R)} end.
receive_body(B) ->
    self() ! B,
    receive <<_:8,R/bits>> -> {byte_size(R),bit_size(R)} end.
generator(B) ->
    [Size] = [{byte_size(R),bit_size(R)} || <<_:8,R/bits>> <- [B]],
    Size.
guarded(<<_:8,R/bits>>, Bytes) when byte_size(R) =:= Bytes ->
    {byte_size(R),bit_size(R)};
guarded(_, _) -> no.
case_guard(B, Bytes) ->
    case B of
        <<_:8,R/bits>> when byte_size(R) =:= Bytes -> {byte_size(R),bit_size(R)};
        _ -> no
    end.
unit3_guard(<<_:8,R/binary-unit:3>>, Bytes) when byte_size(R) =:= Bytes ->
    {byte_size(R),bit_size(R)};
unit3_guard(_, _) -> no.
shifted(<<_:3,R/bits>>) -> {byte_size(R),bit_size(R)}.
dynamic(N, B) -> <<_:N,R/bits>> = B, {byte_size(R),bit_size(R)}.
recursive(<<_:1,R/bits>>, N) when N > 0 -> recursive(R, N - 1);
recursive(<<R/bits>>, 0) -> {byte_size(R),bit_size(R)}.
materialized(<<_:8,R/bits>>) ->
    {byte_size(R),erlang:byte_size(R),byte_size(R),bit_size(R)}.
id(X) -> X.
