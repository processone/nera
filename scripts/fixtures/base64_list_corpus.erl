-module(base64_list_corpus).
-export([start/0]).
start() -> run(base64).
run(M) ->
    corpus(M,4,[],[$A,$h,$9,$+,$/,$-,$_,$=,$*,$\s]),
    lists:foreach(fun(C) -> lists:foreach(fun(I) -> checks(M,I) end,
        [[C],[C,$A,$A,$A],[$A,C,$A,$A],[$A,$A,C,$A],[$A,$A,$A,C],[$Z,$g,$=,$=,C]]) end, lists:seq(-257,512)),
    lists:foreach(fun(C) -> lists:foreach(fun(I) -> checks(M,I) end,
        [[C],[C,$A,$A,$A],[$A,$A,$A,C],[$*,$A,$A,$A,C],[$Z,$g,$=,$=,C],[$Y,$=,$=,$=,C],[$A|C],[$Z,$g,$=,$=|C]]) end,
        [bad,1.0,65.0,[],[65],<<65>>,{},1 bsl 100,-(1 bsl 100)]),
    lists:foreach(fun(N) ->
        B = binary:part(binary:copy(<<0,251,255,239,1,2>>, 44),0,N),
        lists:foreach(fun(O) ->
            E = base64:encode(B,O),
            B = apply(M,decode,[binary_to_list(E),O]),
            checks(M,binary_to_list(E))
        end,options())
    end,lists:seq(0,256)),
    lists:foreach(fun(N) ->
        B = binary:part(binary:copy(list_to_binary(lists:seq(0,255)),257),0,N),
        lists:foreach(fun(O) ->
            E = binary_to_list(base64:encode(B,O)),
            B = apply(M,decode,[E,O]),
            io:format("large_pattern_ok~n")
        end,options())
    end,[8191,8192,8193,65536]),
    lists:foreach(fun(N) ->
        A = lists:duplicate(N,$A),
        checks(M,A), checks(M,A++"Zg=="), checks(M,A++"Y==="),
        checks(M,A++[512]), checks(M,A++[bad]),
        checks(M,A++[65|bad]), checks(M,lists:duplicate(N,$\s)++"Zg==")
    end,[8191,8192,8193,16384,32768]),
    lists:foreach(fun(I) -> lists:foreach(fun(O) -> check(M,I,O) end,
        [#{padding=>invalid},#{mode=>invalid},#{mode=>invalid,padding=>invalid},#{unknown=>ignored},bad,[],42]) end,
        [[],"Zg==","Zg","Y===",[512],[65.0],[65|bad]]),
    io:format("COMPATIBILITY_OK~n"),halt(0).
options() -> [#{},#{padding=>false},#{mode=>urlsafe},#{mode=>urlsafe,padding=>false}].
checks(M,I) -> lists:foreach(fun(O) -> check(M,I,O) end,options()).
check(M,I,O) ->
    R = try apply(M,decode,[I,O]) of V -> {ok,binary_to_list(V)} catch C:E -> {C,E} end,
    io:format("~w~n",[R]).
corpus(M,N,I,Alphabet) -> checks(M,I), case N of
    0 -> ok;
    _ -> lists:foreach(fun(C) -> corpus(M,N-1,I++[C],Alphabet) end,Alphabet)
end.
