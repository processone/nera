%% EXPECT: bitstring_size_badarg_ok
%% Regression: byte_size/1 and bit_size/1 raise badarg for a term that is
%% not a bitstring, as on BEAM, and a guard that calls either on such a term
%% fails instead of seeing a size of 0. Before the fix both returned 0 for
%% lists, atoms and every other term, so `byte_size(X) =< N` matched them.
%% Cowlib 2.22.0's cow_iolists:split/2 relies on that guard failing for the
%% lists inside an iolist, as `pieces/2` below does.
%% Run this same fixture on native Nera and BEAM.
-module(bitstring_size_badarg).
-export([test/0]).

test() ->
    <<_, Tail/binary>> = id(<<1, 2, 3>>),
    <<_:3, Bits/bits>> = id(<<1, 2>>),
    Sizes = [{id(<<>>), 0, 0}, {id(<<"a">>), 1, 8}, {id(<<"abc">>), 3, 24},
             {id(<<1:1>>), 1, 1}, {id(<<1, 5:3>>), 2, 11},
             {Tail, 2, 16}, {Bits, 2, 13},
             {binary:copy(<<"x">>, 100), 100, 800}],
    lists:foreach(fun check_size/1, Sizes),
    NonBitstrings = id([abc, true, [], [1, 2], "abcd", [<<"ab">>], [a | b],
                        0, 42, -1, 1 bsl 70, 1.0, {}, {<<"a">>}, #{},
                        #{a => <<"a">>}, self(), make_ref(),
                        fun() -> ok end, fun id/1]),
    lists:foreach(fun check_badarg/1, NonBitstrings),
    %% A failed guard clears the exception before the next guard alternative.
    recovered = alternative(not_a_bitstring),
    recovered = bit_alternative([1, 2]),
    %% The iolist walk that motivated the fix: "cd" and [] are lists, so the
    %% byte_size/1 clause must not take them as binaries of size 0.
    [{bin, <<"ab">>}, {list, [{char, $c}, {char, $d}]}, {bin, <<"e">>},
     {list, []}, {big, 4}, {char, $!}] =
        pieces(id([<<"ab">>, "cd", <<"e">>, [], <<"long">>, $!]), 3),
    io:format("bitstring_size_badarg_ok~n"),
    ok.

check_size({B, Bytes, Bits}) ->
    Bytes = byte_size(B),
    Bytes = erlang:byte_size(B),
    Bits = bit_size(B),
    Bits = erlang:bit_size(B),
    true = (empty(B) =:= (Bytes =:= 0)),
    true = (qualified_empty(B) =:= (Bytes =:= 0)),
    true = (no_bits(B) =:= (Bits =:= 0)),
    true = (some_bits(B) =:= (Bits > 0)),
    Class = expected_class(Bytes),
    Class = class(B),
    Class = case_class(B),
    Class = if_class(B),
    ok.

check_badarg(X) ->
    badarg = reason(fun() -> byte_size(X) end),
    badarg = reason(fun() -> erlang:byte_size(X) end),
    badarg = reason(fun() -> bit_size(X) end),
    badarg = reason(fun() -> erlang:bit_size(X) end),
    badarg = reason(fun() -> byte_size(X) + 1 end),
    %% Each guard below would be true for a size of 0, so a term that
    %% passes it shows the call returned instead of raising.
    false = empty(X),
    false = qualified_empty(X),
    false = no_bits(X),
    false = qualified_no_bits(X),
    false = small(X),
    false = not_some_bits(X),
    false = empty_orelse(X),
    false = empty_and_term(X),
    other = class(X),
    other = case_class(X),
    other = if_class(X),
    ok.

empty(B) when byte_size(B) =:= 0 -> true;
empty(_) -> false.
qualified_empty(B) when erlang:byte_size(B) == 0 -> true;
qualified_empty(_) -> false.
no_bits(B) when bit_size(B) =:= 0 -> true;
no_bits(_) -> false.
qualified_no_bits(B) when erlang:bit_size(B) =< 0 -> true;
qualified_no_bits(_) -> false.
small(B) when byte_size(B) =< 3, bit_size(B) =< 24 -> true;
small(_) -> false.
some_bits(B) when bit_size(B) > 0 -> true;
some_bits(_) -> false.
%% A raising call fails the whole guard test, so `not` does not turn it true.
not_some_bits(B) when not (bit_size(B) > 0) -> true;
not_some_bits(_) -> false.
empty_orelse(B) when byte_size(B) =:= 0 orelse true -> true;
empty_orelse(_) -> false.
empty_and_term(B) when byte_size(B) =< 0, B =/= ok -> true;
empty_and_term(_) -> false.
alternative(B) when byte_size(B) =:= 0; B =:= not_a_bitstring -> recovered;
alternative(_) -> failed.
bit_alternative(B) when bit_size(B) < 8; is_list(B) -> recovered;
bit_alternative(_) -> failed.

class(B) when byte_size(B) =:= 0 -> empty;
class(B) when byte_size(B) =< 3 -> small;
class(B) when is_bitstring(B) -> large;
class(_) -> other.

case_class(B) ->
    case B of
        _ when byte_size(B) =:= 0 -> empty;
        _ when bit_size(B) =< 24 -> small;
        _ when is_bitstring(B) -> large;
        _ -> other
    end.

if_class(B) ->
    if
        bit_size(B) =:= 0 -> empty;
        byte_size(B) =< 3 -> small;
        is_bitstring(B) -> large;
        true -> other
    end.

expected_class(0) -> empty;
expected_class(N) when N =< 3 -> small;
expected_class(_) -> large.

%% Sort the pieces of an iolist by the clause that takes them: a byte_size/1
%% guard first, then integers, then nested lists. A list must not reach the
%% byte_size/1 clause.
pieces([], _Max) -> [];
pieces([P | Rest], Max) when byte_size(P) =< Max -> [{bin, P} | pieces(Rest, Max)];
pieces([P | Rest], Max) when is_binary(P) -> [{big, byte_size(P)} | pieces(Rest, Max)];
pieces([C | Rest], Max) when is_integer(C) -> [{char, C} | pieces(Rest, Max)];
pieces([L | Rest], Max) -> [{list, pieces(L, Max)} | pieces(Rest, Max)].

reason(F) -> try F() of V -> {unexpected, V} catch error:R -> R end.
id(X) -> X.
