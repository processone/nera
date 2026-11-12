%% NERA-110: binary construction evaluates every segment value and size, left
%% to right and value before size, before it checks any segment, as BEAM
%% does. So <<X:Neg, (1 div Z)>> raises badarith, not the badarg of the
%% earlier negative size. Run this same source on native Nera and BEAM OTP 29.
%% An empty string with a dynamic size is left out: the OTP 29 compiler checks
%% that size where it appears, while its evaluator defers it like every other
%% check, as Nera does: <<"":Atom, X:(1 div Z)>> is badarg compiled, badarith
%% evaluated.
%% EXPECT: binary_construction_order_ok
-module(binary_construction_order).
-export([test/0, readings/0]).

test() ->
    lists:foreach(fun({Name, Expected, Actual}) ->
        case Actual =:= Expected of
            true -> ok;
            false -> erlang:error({wrong_construction_order, Name, Expected, Actual})
        end
    end, readings()),
    io:format("binary_construction_order_ok~n"),
    ok.

%% Each reading is {Name, Expected, Actual}; the exported list gives a native/
%% BEAM transcript. Every value comes through id/1, so erlc builds at runtime.
readings() ->
    [{Name, Expected, result(Fun)} || {Name, Expected, Fun} <- cases()].

cases() ->
    X = id(120), B = id(<<"xy">>), Z = id(0), N1 = id(1), N4 = id(4),
    N8 = id(8), Neg = id(-1), Atom = id(sixteen),
    [
        %% A later segment's failing expression runs before an earlier
        %% segment's check, whatever kind of check that is.
        {size_then_value, {error, badarith}, fun() -> <<X:Neg, (1 div Z)>> end},
        {size_then_size, {error, badarith}, fun() -> <<X:Neg, X:(1 div Z)>> end},
        {binary_then_value, {error, badarith}, fun() -> <<X/binary, (1 div Z)>> end},
        {atom_size_then_value, {error, badarith}, fun() -> <<X:Atom, (1 div Z)>> end},
        {float_size_then_value, {error, badarith}, fun() -> <<X:Neg/float, (1 div Z)>> end},
        {float_width_then_value, {error, badarith}, fun() -> <<X:N8/float, (1 div Z)>> end},
        {sized_binary_then_value, {error, badarith}, fun() -> <<B:Neg/binary, (1 div Z)>> end},
        {short_binary_then_value, {error, badarith}, fun() -> <<B:N4/binary, (1 div Z)>> end},
        {string_size_then_value, {error, badarith}, fun() -> <<"ab":Neg, (1 div Z)>> end},
        {string_binary_then_value, {error, badarith}, fun() -> <<"ab"/binary, (1 div Z)>> end},
        {integer_value_then_value, {error, badarith}, fun() -> <<Atom, (1 div Z)>> end},
        {utf_value_then_value, {error, badarith}, fun() -> <<Neg/utf8, (1 div Z)>> end},
        {comprehension, {error, badarith}, fun() -> << <<X:Neg, (1 div Z)>> || _ <- id([1]) >> end},
        %% Evaluation itself stays left to right, value before size.
        {earlier_value_first, {error, badarg}, fun() -> <<(element(1, Z)), (1 div Z)>> end},
        {value_before_size, {error, badarg}, fun() -> <<(element(1, Z)):(1 div Z)>> end},
        {size_before_next_value, {error, badarg}, fun() -> <<X:(element(1, Z)), (1 div Z)>> end},
        {trace_failing, {ok, {{error, badarg}, [v1, s1, v2, s2, v3]}}, fun() ->
            traced(fun() ->
                <<(t(v1, X)):(t(s1, Neg)), (t(v2, X)):(t(s2, 8)), (t(v3, B))/binary>>
            end) end},
        {trace_built, {ok, {{ok, <<120, 0, 120, "xy">>}, [v1, s1, v2, s2, v3]}}, fun() ->
            traced(fun() ->
                <<(t(v1, X)):(t(s1, 8)), (t(v2, X)):(t(s2, 16)), (t(v3, B))/binary>>
            end) end},
        {valid, {ok, <<120, 1>>}, fun() -> <<X:N8, (1 div N1)>> end},
        %% The same constructions with the handler in their own function.
        {try_size_then_value, {ok, {caught, badarith}}, fun() -> order_try(size_then_value, Z) end},
        {try_size_then_size, {ok, {caught, badarith}}, fun() -> order_try(size_then_size, Z) end},
        {try_binary_then_value, {ok, {caught, badarith}}, fun() -> order_try(binary_then_value, Z) end},
        {try_atom_size_then_value, {ok, {caught, badarith}}, fun() -> order_try(atom_size_then_value, Z) end},
        {try_float_size_then_value, {ok, {caught, badarith}}, fun() -> order_try(float_size_then_value, Z) end},
        {try_sized_binary_then_value, {ok, {caught, badarith}}, fun() -> order_try(sized_binary_then_value, Z) end},
        {try_string_size_then_value, {ok, {caught, badarith}}, fun() -> order_try(string_size_then_value, Z) end},
        {try_string_binary_then_size, {ok, {caught, badarith}}, fun() -> order_try(string_binary_then_size, Z) end},
        %% Once every expression succeeds, the earlier check still fails.
        {try_check_after_values, {ok, {caught, badarg}}, fun() -> order_try(size_then_value, N1) end}
    ].

result(Fun) ->
    try {ok, Fun()} catch error:Reason -> {error, Reason} end.

%% Z is 0 for the failing cases; each earlier segment's check would fail.
order_try(Kind, Z) ->
    X = id(120), B = id(<<"xy">>), Neg = id(-1), Atom = id(sixteen),
    try
        case Kind of
            size_then_value -> <<X:Neg, (1 div Z)>>;
            size_then_size -> <<X:Neg, X:(1 div Z)>>;
            binary_then_value -> <<X/binary, (1 div Z)>>;
            atom_size_then_value -> <<X:Atom, (1 div Z)>>;
            float_size_then_value -> <<X:Neg/float, (1 div Z)>>;
            sized_binary_then_value -> <<B:Neg/binary, (1 div Z)>>;
            string_size_then_value -> <<"ab":Neg, (1 div Z)>>;
            string_binary_then_size -> <<"ab"/binary, X:(1 div Z)>>
        end
    of Built -> {built, Built}
    catch error:Reason -> {caught, Reason}
    end.

%% Records each value and size as it is evaluated.
traced(Fun) ->
    put(order_trace, []),
    Result = result(Fun),
    {Result, lists:reverse(erase(order_trace))}.

t(Tag, Value) ->
    put(order_trace, [Tag | get(order_trace)]),
    Value.

id(X) -> X.
