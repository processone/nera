-module(bench).
-export([start/0]).
start() ->
    lists:foreach(fun(N) ->
        B = binary:copy(<<251,255,239,0,1,2>>, N div 6),
        lists:foreach(fun({Mode,Pad}) ->
            O = #{mode => Mode,padding => Pad},
            E = base64:encode(B,O),
            lists:foreach(fun({F,Input}) ->
                Iters = max(4, 1048576 div N),
                %% Warm up both implementations. Report repeated opposite orders.
                loop(base64,F,Input,O,3,0), loop(base64_rust,F,Input,O,3,0),
                lists:foreach(fun(R) ->
                    Ms = case R rem 2 of 0 -> [base64_rust,base64]; _ -> [base64,base64_rust] end,
                    lists:foreach(fun(M) -> measure(M,F,Input,O,N,Iters,Mode,Pad,R) end,Ms)
                end, lists:seq(1,7))
            end,[{encode,B},{decode,E}])
        end,[{standard,true},{urlsafe,false}])
    end,[12,1026,16386,65538]),
    io:format("BENCH_OK~n"), halt(0).
measure(M,F,I,O,N,BaseCount,Mode,Pad,R) ->
    Count = case M of base64_rust -> BaseCount * 32; _ -> BaseCount end,
    nera_base64_native:start(),
    T0 = erlang:monotonic_time(microsecond),
    Sum = loop(M,F,I,O,Count,0),
    T1 = erlang:monotonic_time(microsecond),
    {Calls,Bytes,HeapCalls,HeapBytes} = nera_base64_native:stop(),
    io:format("ROW ~p ~p ~p ~p ~p ~p ~p ~p ~p ~p ~p ~p ~p~n",
        [M,F,N,Mode,Pad,R,Count,T1-T0,Calls,Bytes,HeapCalls,HeapBytes,Sum]).
loop(_,_,_,_,0,Sum) -> Sum;
loop(M,F,I,O,N,Sum) ->
    Result = apply(M,F,[I,O]),
    loop(M,F,I,O,N-1,Sum+byte_size(Result)).
