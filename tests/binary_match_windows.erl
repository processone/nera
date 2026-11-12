-module(binary_match_windows).
-export([test/0]).

test() ->
    lists:foreach(fun(N) ->
        Bytes = list_to_binary(lists:seq(0,N-1)),
        lists:foreach(fun(P) ->
            lists:foreach(fun(Count) ->
                <<Part:Count/bitstring,_/bitstring>> = Bytes,
                Expected = <<5:P,Part/bitstring,3:2>>,
                true = (Expected =:= append(Bytes,Count,P)),
                true = (Part =:= tail(<<5:P,Part/bitstring>>,P)),
                {Pid,Ref} = spawn_monitor(fun() ->
                    %% Force the escaped window to cross a process boundary.
                    receive {From,Input} -> From ! {self(),tail(Input,P)} end
                end),
                Pid ! {self(),<<5:P,Part/bitstring>>},
                receive {Who,Actual} ->
                    case {Who=:=Pid,Actual=:=Part} of
                        {true,true} -> ok;
                        _ -> error({window_mismatch,N,P,Count,Who,Pid,Actual,Part})
                    end
                after 2000 -> error(missing_bits) end,
                receive {'DOWN',Ref,process,Pid,normal} -> ok after 2000 -> error(missing_down) end
            end,[0,1,7,8,13,N*8-1,N*8])
        end,lists:seq(0,7))
    end,[2,16,65,256]),
    true = (duplicate(<<1,2,3>>) =:= {2,2}),
    Data = <<1,2,3,4,5>>,
    15 = sum(Data),
    io:format("matching windows passed~n"),
    ok.
append(B,N,P) -> <<5:P,B:N/bitstring,3:2>>.
tail(B,P) -> <<_:P,T/bitstring>> = B, T.
sum(<<>>) -> 0;
sum(<<H:8,T/bitstring>>) -> H + sum(T).

duplicate(<<_:8,T/bitstring>>) -> both(T,T).
both(<<A:8,_/bitstring>>,<<B:8,_/bitstring>>) -> {A,B}.
