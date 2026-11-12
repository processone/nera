%% EXPECT: binary closure captures passed
-module(closure_binary_captures).
-export([test/0]).

test() ->
    lists:foreach(fun(N) ->
        Bytes = list_to_binary(lists:seq(0,N-1)),
        lists:foreach(fun(P) ->
            lists:foreach(fun(Count) ->
                <<Expected:Count/bitstring,_/bitstring>> = Bytes,
                Input = <<5:P,Expected/bitstring>>,
                F = capture(Input,P),
                C = capture_case(Input,P),
                _ = churn(8000,0),
                true = (F() =:= Expected),
                true = (C() =:= Expected),
                {Pid,Ref} = spawn_monitor(fun() ->
                    receive {From,G} ->
                        _ = churn(8000,0),
                        From ! {self(),G()}
                    end
                end),
                Pid ! {self(),F},
                receive {Who,Actual} ->
                    true = (Who =:= Pid),
                    true = (Actual =:= Expected)
                after 2000 -> error(missing_capture) end,
                receive {'DOWN',Ref,process,Pid,normal} -> ok
                after 2000 -> error(missing_down) end
            end,[0,1,7,8,13,N*8-1,N*8])
        end,lists:seq(0,7))
    end,[2,16,65,256]),
    Input = <<5:3,1,2,3,4>>,
    {G,T} = capture_and_use(Input),
    true = (T =:= <<1,2,3,4>>),
    true = (G() =:= T),
    Nested = capture_nested(Input),
    Inner = Nested(),
    true = (Inner() =:= T),
    Named = capture_named(Input),
    true = (Named(20) =:= T),
    Table = ets:new(captures,[set]),
    true = ets:insert(Table,{key,G}),
    [{key,Stored}] = ets:lookup(Table,key),
    _ = churn(8000,0),
    true = (Stored() =:= T),
    true = ets:delete(Table),
    io:format("binary closure captures passed~n"),
    ok.

capture(B,P) -> <<_:P,T/bitstring>> = B, fun() -> T end.
capture_case(B,P) -> case B of <<_:P,T/bitstring>> -> fun() -> T end end.
capture_and_use(<<_:3,T/bitstring>>) -> {fun() -> T end,T}.
capture_nested(<<_:3,T/bitstring>>) -> fun() -> fun() -> T end end.
capture_named(<<_:3,T/bitstring>>) -> fun Loop(0) -> T; Loop(N) -> Loop(N-1) end.

%% Force allocation and scheduler GC checks without depending on a GC BIF.
churn(0,A) -> A;
churn(N,A) ->
    H = erlang:phash2({N,N+1,N+2,N+3,N+4,N+5,N+6,N+7}),
    churn(N-1,A+H).
