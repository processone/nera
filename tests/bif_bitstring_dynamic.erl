%% EXPECT: bif_bitstring_dynamic_ok
%% Regression: dynamic bit_size/1 and is_bitstring/1 calls must
%% agree with direct calls, not raise undef. The runtime fallback table
%% used to omit both BIFs. Run this same fixture on native Nera and BEAM.
-module(bif_bitstring_dynamic).
-export([test/0]).

test() ->
    SizeCalls = calls(bit_size),
    TypeCalls = calls(is_bitstring),
    <<_, Tail/binary>> = id(<<1, 2, 3, 4>>),
    <<_:3, Bits/bits>> = id(<<1, 2, 3>>),
    Big = id(binary:copy(<<"x">>, 1000)),
    <<_:5/binary, Middle:990/binary, _/binary>> = Big,
    Bitstrings = id([<<>>, <<"a">>, <<"abc">>, <<1:1>>, <<1:7>>,
                     <<1, 5:3>>, <<1:9>>, Tail, Bits, Big, Middle,
                     <<Big/binary, 1:1>>]),
    lists:foreach(fun(B) ->
        %% Direct size semantics have their own bitstring_size_badarg fixture;
        %% this regression owns parity of the dynamic entry points.
        check_calls(bit_size, SizeCalls, B, {ok, bit_size(B)}),
        check_calls(is_bitstring, TypeCalls, B, {ok, true})
    end, Bitstrings),

    NonBitstrings = id([abc, true, [], [1, 2], "abcd", [<<"ab">>], [a | b],
                        0, 42, -1, 1 bsl 70, 1.0, {}, {<<"a">>}, #{},
                        #{a => <<"a">>}, self(), make_ref(),
                        fun() -> ok end, fun id/1]),
    lists:foreach(fun(X) ->
        check_calls(bit_size, SizeCalls, X, {error, badarg}),
        check_calls(is_bitstring, TypeCalls, X, {ok, false})
    end, NonBitstrings),

    %% Real higher-order consumers of auto-imported and remote fun references.
    [0, 24, 9, 1] = lists:map(fun bit_size/1, id([<<>>, <<"abc">>, <<1:9>>, <<1:1>>])),
    [true, false, true] = lists:map(fun erlang:is_bitstring/1,
                                   id([<<1, 5:3>>, [1, 2], <<>>])),
    %% A caught badarg does not prevent a later dynamic call from succeeding.
    Remote = fun erlang:bit_size/1,
    {error, badarg} = outcome(Remote, id(not_bits)),
    11 = apply(erlang, bit_size, [id(<<1, 5:3>>)]),
    io:format("bif_bitstring_dynamic_ok~n"),
    ok.

calls(bit_size) ->
    Mod = id(erlang),
    [{local_fun, fun bit_size/1},
     {remote_fun, fun erlang:bit_size/1},
     {variable_module, fun(B) -> Mod:bit_size(B) end}
     | dynamic_calls(bit_size)];
calls(is_bitstring) ->
    Mod = id(erlang),
    [{local_fun, fun is_bitstring/1},
     {remote_fun, fun erlang:is_bitstring/1},
     {variable_module, fun(B) -> Mod:is_bitstring(B) end}
     | dynamic_calls(is_bitstring)].

dynamic_calls(Name) ->
    Mod = id(erlang),
    Function = id(Name),
    Dynamic = fun Mod:Function/1,
    [{apply, fun(B) -> apply(erlang, Name, [B]) end},
     {qualified_apply, fun(B) -> erlang:apply(erlang, Name, [B]) end},
     {variable_apply, fun(B) -> apply(Mod, Function, [B]) end},
     {variable_function, fun(B) -> erlang:Function(B) end},
     {variable_module_function, fun(B) -> Mod:Function(B) end},
     {dynamic_fun, Dynamic},
     {apply_fun, fun(B) -> apply(Dynamic, [B]) end},
     {apply_local_fun, fun(B) -> apply(local_fun(Name), [B]) end},
     {apply_remote_fun, fun(B) -> apply(remote_fun(Name), [B]) end}].

local_fun(bit_size) -> fun bit_size/1;
local_fun(is_bitstring) -> fun is_bitstring/1.
remote_fun(bit_size) -> fun erlang:bit_size/1;
remote_fun(is_bitstring) -> fun erlang:is_bitstring/1.

check_calls(Name, Calls, X, Expected) ->
    Results = [{Form, outcome(F, X)} || {Form, F} <- Calls],
    ExpectedResults = [{Form, Expected} || {Form, _} <- Calls],
    {Name, X, ExpectedResults} = {Name, X, Results},
    ok.

outcome(F, X) -> try F(X) of V -> {ok, V} catch error:R -> {error, R} end.
id(X) -> X.
