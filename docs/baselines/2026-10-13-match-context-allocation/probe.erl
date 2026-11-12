-module(probe).
-export([start/0]).
start() ->
    lists:foreach(fun(Size) ->
        B = binary:copy(<<251,255,239,0,1,2>>, Size div 6),
        Encoded = base64:encode(B,#{}),
        lists:foreach(fun({Op,Input}) ->
            loop(Op,Input,2,0),
            lists:foreach(fun(Repeat) -> measure(Op,Input,Size,Repeat) end,lists:seq(1,7))
        end,[{encode,B},{decode,Encoded},{scan,B}])
    end,[1026,16386,65538]),
    io:format("PROBE_OK~n"), halt(0).
measure(Op,Input,Size,Repeat) ->
    Count = max(4,262144 div Size),
    nera_base64_native:start(),
    T0 = erlang:monotonic_time(microsecond),
    Sum = loop(Op,Input,Count,0),
    T1 = erlang:monotonic_time(microsecond),
    {Calls,Bytes,HeapCalls,HeapBytes,Refills,RefillBytes,GcCalls,GcBytes} = nera_base64_native:stop(),
    io:format("ROW ~p ~p ~p ~p ~p ~p ~p ~p ~p ~p ~p ~p ~p ~p~n",
        [Op,Size,Repeat,Count,T1-T0,Calls,Bytes,HeapCalls,HeapBytes,Refills,RefillBytes,GcCalls,GcBytes,Sum]).
loop(_,_,0,A) -> A;
loop(Op,Input,N,A) -> loop(Op,Input,N-1,A+operation(Op,Input)).
operation(encode,B) -> byte_size(base64:encode(B,#{}));
operation(decode,B) -> byte_size(base64:decode(B,#{}));
operation(scan,B) -> scan(B,0).
scan(<<>>,A) -> A;
scan(<<C,Rest/binary>>,A) -> scan(Rest,A+C).
