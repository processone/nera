-module(differential).
-export([start/0]).
start() ->
    %% A native result proves the tested path did not silently fall back.
    <<"Zg==">> = nera_base64_native:encode(<<"f">>, false, true),
    <<"f">> = nera_base64_native:decode(<<"Zh==">>, false, true),
    fallback = nera_base64_native:decode(<<"***=">>, false, true),
    All = list_to_binary(lists:seq(0,255)),
    Options = [#{}, #{mode => urlsafe}, #{padding => false},
               #{mode => urlsafe, padding => false},
               #{mode => invalid, padding => invalid}, #{unknown => ignored}],
    lists:foreach(fun(N) ->
        B = binary:part(All, 0, N),
        lists:foreach(fun(O) ->
            check(encode,B,O), check(encode,binary_to_list(B),O),
            E = base64:encode(B,O),
            check(decode,E,O), check(decode,binary_to_list(E),O),
            check(decode,<<" \t",E/binary,"\r\n">>,O),
            B = base64_rust:decode(base64_rust:encode(B,O),O)
        end, Options)
    end, lists:seq(0,256)),
    %% Exhaustive short strings over representative symbol classes, both modes.
    corpus(4, <<>>, [$A,$Z,$h,$9,$+,$/,$-,$_,$=,$*,$\s]),
    lists:foreach(fun(C) -> lists:foreach(fun(B) ->
        lists:foreach(fun(O) -> check(decode,B,O) end, Options)
    end, [<<C,"AAA">>, <<"A",C,"AA">>, <<"AA",C,"A">>, <<"AAA",C>>]) end,
        lists:seq(0,255)),
    Inputs = [42, bad, [], [256], [-1], [1.0], [a], [[1]], [1|2],
              <<1:1>>, <<>>, <<"A">>, <<"Zg==*">>, <<"***=">>,
              <<"Y===">>, <<"Zg===">>, <<"Zg==Zg==">>,
              [90,103,61,61,a], [90,103,61,61|a]],
    lists:foreach(fun(I) -> lists:foreach(fun(O) ->
        check(encode,I,O), check(decode,I,O)
    end, [#{} ,#{mode => urlsafe, padding => false}, [], bad, 42]) end, Inputs),
    %% Large refcounted data, subbinaries, repeated calls with collection pressure.
    Large = binary:copy(All, 256),
    lists:foreach(fun(_) -> lists:foreach(fun(O) ->
        check(encode,Large,O), check(decode,base64:encode(Large,O),O)
    end, Options) end, lists:seq(1,8)),
    Slice = binary:part(Large, 37, 8193),
    check(encode,Slice,#{}),
    EFun = fun base64_rust:encode/2, DFun = fun base64_rust:decode/2,
    <<"f">> = DFun(EFun(<<"f">>,#{}),#{}),
    <<"Zg==">> = apply(base64_rust,encode,[<<"f">>,#{}]),
    <<"f">> = apply(base64_rust,decode,[<<"Zg==">>,#{}]),
    io:format("DIFFERENTIAL_OK~n"), halt(0).
corpus(N, B, Alphabet) ->
    lists:foreach(fun(O) -> check(decode,B,O) end,
        [#{}, #{padding => false}, #{mode => urlsafe}, #{mode => urlsafe,padding => false}]),
    case N of 0 -> ok; _ -> lists:foreach(fun(C) -> corpus(N-1,<<B/binary,C>>,Alphabet) end,Alphabet) end.
check(F,I,O) ->
    A = outcome(base64,F,I,O), B = outcome(base64_rust,F,I,O),
    case A =:= B of true -> ok; false -> io:format("MISMATCH ~p~n",[{F,I,O,A,B}]), halt(1) end.
outcome(M,F,I,O) -> try apply(M,F,[I,O]) of V -> {ok,V} catch C:R -> {C,R} end.
