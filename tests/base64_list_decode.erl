%% EXPECT: ok
%% Public list decoding: values and exception reasons shared with BEAM.
-module(base64_list_decode).
-export([test/0]).
test() ->
    {error,badarg} = outcome([512], #{}),
    {error,badarg} = outcome([65.0], #{}),
    {error,badarg} = outcome([$*,$A,$A,$A,bad], #{}),
    {error,badarith} = outcome("Y===", #{}),
    {error,{case_clause,0}} = outcome("Zg== A", #{}),
    {error,missing_padding} = outcome("Zg", #{}),
    {error,function_clause} = outcome([$Z,$g,$=,$=|bad], #{}),
    %% The current OTP lookup covers both alphabets, including these integers.
    {ok,<<0,0,0>>} = outcome([321,321,321,321], #{}),
    {ok,<<0,0,0>>} = outcome([-191,-191,-191,-191], #{mode => urlsafe}),
    {ok,<<"f">>} = outcome(" Z\tg=\r=\n", #{}),
    {ok,<<"f">>} = outcome("Zh", #{padding => false}),
    lists:foreach(fun(N) ->
        B = binary:part(binary:copy(list_to_binary(lists:seq(0,255)),257),0,N),
        lists:foreach(fun(O) ->
            B = base64:decode(binary_to_list(base64:encode(B,O)),O)
        end,[#{},#{mode=>urlsafe},#{padding=>false},#{mode=>urlsafe,padding=>false}])
    end,[0,1,2,3,8191,8192,8193,65536]),
    lists:foreach(fun(N) ->
        Lead = lists:duplicate(8192-N,$\s),
        {ok,<<"f">>} = outcome(Lead++"Z\tg=\r=\n", #{}),
        {error,badarith} = outcome(Lead++"Y===", #{})
    end,[0,1,2,3,4]),
    Space = lists:duplicate(32768,$\s),
    {ok,<<"f">>} = outcome(Space++"Zg==", #{}),
    {error,badarith} = outcome(Space++"Y===", #{}),
    {error,badarg} = outcome(Space++[512], #{}),
    {error,function_clause} = outcome(Space++[$A|bad], #{}),
    F = fun base64:decode/2,
    <<"f">> = F("Zg==",#{}),
    io:format("ok~n").
outcome(I,O) -> try base64:decode(I,O) of V -> {ok,V} catch C:R -> {C,R} end.
