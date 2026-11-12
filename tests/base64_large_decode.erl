%% EXPECT: ok
%% Runs unchanged against OTP's base64 module.
-module(base64_large_decode).
-export([test/0, single_scheduler/0]).
test() -> vectors(), fairness(false), io:format("ok~n").
single_scheduler() -> fairness(true), io:format("ok~n").
vectors() ->
    lists:foreach(fun(N) ->
        Raw = binary:copy(<<251,255,239>>, N),
        lists:foreach(fun({Mode, Symbol}) ->
            Prefix = binary:copy(Symbol, N),
            lists:foreach(fun({Suffix, Tail, Pad}) ->
                Wrapped = <<42," \t",Prefix/binary,Suffix/binary,"\r\n",43>>,
                Input = binary:part(Wrapped, 1, byte_size(Wrapped)-2),
                Expected = <<Raw/binary,Tail/binary>>,
                Opts = #{mode => Mode, padding => Pad},
                Expected = base64:decode(Input, Opts),
                Expected = apply(base64, decode, [Input, Opts]),
                F = fun base64:decode/2,
                Expected = F(Input, Opts),
                Expected = try base64:decode(Input, Opts) catch C:R -> error({C,R}) end,
                42 = binary:at(Wrapped, 0)
            end, [{<<>>,<<>>,true}, {<<"Zh==">>,<<"f">>,true},
                  {<<"Zm9=">>,<<"fo">>,true}, {<<"Zg">>,<<"f">>,false},
                  {<<"Zg=">>,<<"f">>,false}, {<<"Zm8">>,<<"fo">>,false}])
        end, [{standard,<<"+//v">>},{urlsafe,<<"-__v">>}])
    end, [0,1,8191,8192,8193,262145]),
    %% All four quartet positions across a slice boundary, including whitespace.
    Space = binary:copy(<<" ">>, 32768),
    lists:foreach(fun(N) ->
        Lead = binary:part(Space, 0, 32768-N),
        <<"f">> = base64:decode(<<Lead/binary,"Z\tg=\r=\n">>, #{}),
        {error,badarith} = outcome(<<Lead/binary,"Y===">>)
    end, [0,1,2,3,4]),
    BigSpace = binary:copy(<<" ">>, 3*1024*1024),
    {error,{case_clause,bad}} = outcome(<<"Zg==",BigSpace/binary,"*">>),
    {error,{case_clause,0}} = outcome(<<"Zg==",BigSpace/binary,"A">>),
    {error,{case_clause,eq}} = outcome(<<"Zg==",BigSpace/binary,"=">>),
    {error,missing_padding} = outcome(<<BigSpace/binary,"**">>),
    {error,function_clause} = outcome(<<BigSpace/binary,"A">>),
    ok.
outcome(Input) -> try base64:decode(Input, #{}) of V -> {ok,V} catch C:R -> {C,R} end.
fairness(Strict) ->
    Space = binary:copy(<<" ">>, 3*1024*1024),
    Data = binary:copy(<<"AAAA">>, 1024*1024),
    lists:foreach(fun({Input, Expected}) -> compete(Input, Expected, Strict) end,
        [{Space,{ok,<<>>}}, {<<Space/binary,"Zg==">>,{ok,<<"f">>}},
         {<<Space/binary,"Y===">>,{error,badarith}},
         {<<"Zg==",Space/binary,"*">>,{error,{case_clause,bad}}},
         {Data,{ok,binary:copy(<<0>>,3*1024*1024)}}]).
compete(Input, Expected, Strict) ->
    Parent = self(),
    Observer = spawn(fun() -> Parent ! ready, receive go -> Parent ! observed end end),
    receive ready -> ok after 10000 -> error(observer_not_ready) end,
    Worker = spawn(fun() -> receive go ->
        Observer ! go,
        Expected = outcome(Input),
        Parent ! decoded
    end end),
    Worker ! go,
    receive
        observed -> receive decoded -> ok after 10000 -> error(decoder_timeout) end;
        decoded when Strict =:= false -> receive observed -> ok after 10000 -> error(observer_timeout) end;
        decoded -> error(observer_starved)
    after 10000 -> error(no_progress)
    end.
