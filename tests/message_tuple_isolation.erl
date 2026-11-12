%% EXPECT: message_tuple_isolation ok
-module(message_tuple_isolation).
-export([test/0]).

test() ->
    lists:foreach(fun(_) -> exchange() end, lists:seq(1,20)),
    L=lists:seq(1,100),
    {self_message,L,L} = (self() ! {self_message,L,L}),
    receive {self_message,L,L} -> ok after 10000 -> error(self_timeout) end,
    try bad_destination() ! {bad,L,L} of _ -> error(expected_badarg)
    catch error:badarg -> ok end,
    io:format("message_tuple_isolation ok~n").
bad_destination() -> 42.
exchange() ->
    Parent=self(),
    {Pid,Ref}=spawn_monitor(fun() -> send_payload(Parent) end),
    %% Leave the payload queued until the sender has released its heap.
    receive {'DOWN',Ref,process,Pid,normal} -> ok after 10000 -> error(down_timeout) end,
    receive {payload,M2,L2,L3} ->
        true=(L2=:=lists:seq(1,2048)), L2=L3, L2=maps:get(list,M2),
        true=(maps:get(binary,M2)=:=binary:copy(<<17,42,99>>,1000)),
        true=(maps:get(big,M2)=:=(1 bsl 130))
    after 10000 -> error(payload_timeout) end.

%% A named function is optimized by the tuple/send fusion pass. Keep the send
%% here rather than in the closure so this exercises nera_rt_send_tuple.
send_payload(Parent) ->
    L=lists:seq(1,2048),
    B=binary:copy(<<17,42,99>>,1000),
    Big=1 bsl 130,
    M=#{list=>L,binary=>B,big=>Big},
    {payload,M,L,L} = (Parent ! {payload,M,L,L}).
