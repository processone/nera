%% EXPECT: bif_byte_size_dynamic_ok
%% Regression: byte_size/1 reached through apply/2,3, a variable module or
%% function name, or a fun reference returns the byte count, rounding a
%% partial final byte up, and raises badarg for a term that is not a
%% bitstring, as on BEAM. Before the fix every such call returned the bit
%% count, so apply(erlang, byte_size, [<<"abc">>]) was 24. Direct and
%% erlang:-qualified calls were already correct and serve as the reference.
%% Run this same fixture on native Nera and BEAM.
-module(bif_byte_size_dynamic).
-export([test/0]).

test() ->
    <<_, Tail/binary>> = id(<<1, 2, 3, 4>>),
    <<_:3, Bits/bits>> = id(<<1, 2, 3>>),
    Big = id(binary:copy(<<"x">>, 1000)),
    <<_:5/binary, Middle:990/binary, _/binary>> = Big,
    Sizes = [{id(<<>>), 0}, {id(<<"a">>), 1}, {id(<<"abc">>), 3},
             {id(<<1:1>>), 1}, {id(<<1:7>>), 1}, {id(<<1, 5:3>>), 2},
             {id(<<1:9>>), 2}, {Tail, 3}, {Bits, 3}, {Big, 1000},
             {Middle, 990}, {id(<<Big/binary, 1:1>>), 1001}],
    lists:foreach(fun check_size/1, Sizes),

    %% A fun reference passed to a library function, as BEAM code often does.
    [0, 3, 2, 1] = lists:map(fun byte_size/1, id([<<>>, <<"abc">>, <<1:9>>, <<1:1>>])),
    [3, 2] = lists:map(fun erlang:byte_size/1, id([<<"abc">>, <<1, 5:3>>])),
    6 = lists:foldl(fun(B, Acc) -> apply(erlang, byte_size, [B]) + Acc end,
                    0, id([<<"ab">>, <<"c">>, <<1:17>>])),
    true = is_function(fun erlang:byte_size/1, 1),
    true = is_function(fun byte_size/1, 1),

    NonBitstrings = id([abc, true, [], [1, 2], "abcd", [<<"ab">>], [a | b],
                        0, 42, -1, 1 bsl 70, 1.0, {}, {<<"a">>}, #{},
                        #{a => <<"a">>}, self(), make_ref(),
                        fun() -> ok end, fun id/1]),
    lists:foreach(fun check_badarg/1, NonBitstrings),

    %% The raise is an ordinary exception: code after a caught one runs.
    {caught, 3} = after_badarg(id([1, 2]), id(<<"abc">>)),

    io:format("bif_byte_size_dynamic_ok~n"),
    ok.

%% Each call form's result sits in one list, so a mismatch shows the term
%% and every form's answer together.
check_size({B, Bytes}) ->
    Mod = id(erlang),
    Fun = id(byte_size),
    Local = fun byte_size/1,
    Remote = fun erlang:byte_size/1,
    Dynamic = fun Mod:Fun/1,
    Results = [byte_size(B),
               erlang:byte_size(B),
               apply(erlang, byte_size, [B]),
               erlang:apply(erlang, byte_size, [B]),
               apply(id(erlang), id(byte_size), [B]),
               apply(Local, [B]),
               apply(Remote, [B]),
               apply(Dynamic, [B]),
               Mod:byte_size(B),
               Mod:Fun(B),
               erlang:Fun(B),
               Local(B),
               Remote(B),
               Dynamic(B),
               (id(Local))(B),
               (id(Remote))(B)],
    Expected = [Bytes || _ <- Results],
    {B, Expected} = {B, Results},
    ok.

check_badarg(X) ->
    Mod = id(erlang),
    Fun = id(byte_size),
    Local = fun byte_size/1,
    Remote = fun erlang:byte_size/1,
    Dynamic = fun Mod:Fun/1,
    Calls = [fun() -> byte_size(X) end,
             fun() -> apply(erlang, byte_size, [X]) end,
             fun() -> erlang:apply(erlang, byte_size, [X]) end,
             fun() -> apply(id(erlang), id(byte_size), [X]) end,
             fun() -> apply(Local, [X]) end,
             fun() -> apply(Remote, [X]) end,
             fun() -> apply(Dynamic, [X]) end,
             fun() -> Mod:byte_size(X) end,
             fun() -> Mod:Fun(X) end,
             fun() -> erlang:Fun(X) end,
             fun() -> Local(X) end,
             fun() -> Remote(X) end,
             fun() -> Dynamic(X) end,
             fun() -> apply(erlang, byte_size, [X]) + 1 end,
             fun() -> Remote(X) + 1 end],
    Expected = [badarg || _ <- Calls],
    {X, Expected} = {X, [reason(C) || C <- Calls]},
    ok.

after_badarg(NotBin, Bin) ->
    Remote = fun erlang:byte_size/1,
    badarg = reason(fun() -> Remote(NotBin) end),
    {caught, apply(erlang, byte_size, [Bin])}.

reason(F) -> try F() of V -> {unexpected, V} catch error:R -> R end.
id(X) -> X.
