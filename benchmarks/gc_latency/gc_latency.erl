-module(gc_latency).
-export([small/0, large/0, huge/0, spaces/0, late_error/0, idle/0, waits/0]).
small() -> run(65536,data).
large() -> run(1048576,data).
huge() -> run(8388608,data).
spaces() -> run(1048576,spaces).
late_error() -> run(1048576,late_error).
idle() -> run(0,idle).
run(N,Kind) ->
 I = case Kind of
  data -> lists:duplicate(N,$A);
  spaces -> lists:duplicate(N,$\s);
  late_error -> lists:duplicate(N,$\s)++"Y===";
  idle -> []
 end,
 Parent = self(), Probe = spawn(fun() -> probe() end),
 Worker = spawn(fun() -> receive go ->
  K = until(I,Kind,N,erlang:monotonic_time(microsecond)+1000000,0), Parent ! {done,K}
 end end),
 Start = erlang:monotonic_time(microsecond), Worker ! go,
 collect(Probe,Start,[],false).
until(I,Kind,N,Deadline,K) ->
 case Kind of
  data -> R = base64:decode(I,#{}), true = byte_size(R) =:= (N div 4)*3;
  spaces -> <<>> = base64:decode(I,#{});
  late_error -> try base64:decode(I,#{}) of _ -> error(expected_error) catch error:badarith -> ok end;
  idle -> receive after 1 -> ok end
 end,
 case erlang:monotonic_time(microsecond) >= Deadline of true -> K+1; false -> until(I,Kind,N,Deadline,K+1) end.
probe() -> receive {ping,P} -> P ! pong,probe(); stop -> ok end.
collect(Probe,Start,Samples,{true,K}) ->
 Probe ! stop,
 io:format("elapsed ~p~noperations ~p~n",[erlang:monotonic_time(microsecond)-Start,K]),
 lists:foreach(fun(X) -> io:format("~p~n",[X]) end,Samples);
collect(Probe,Start,Samples,Done) ->
 T = erlang:monotonic_time(microsecond), Probe ! {ping,self()},wait(Probe,Start,T,Samples,Done).
wait(Probe,Start,T,Samples,Done) -> receive
 {done,K} -> wait(Probe,Start,T,Samples,{true,K});
 pong -> collect(Probe,Start,[erlang:monotonic_time(microsecond)-T|Samples],Done)
end.

%% Keep a 16 MiB live list over repeated receive timeouts. Every wait has
%% the same pointer roots; changing an immediate counter is not heap garbage.
waits() ->
 I = lists:duplicate(1048576,$A),
 Parent = self(), Probe = spawn(fun() -> probe() end),
 Worker = spawn(fun() -> receive go ->
  waits_loop(I,erlang:monotonic_time(microsecond)+1000000,0,Parent)
 end end),
 Start = erlang:monotonic_time(microsecond), Worker ! go,
 collect(Probe,Start,[],false).
waits_loop(I,Deadline,K,Parent) ->
 receive after 1 -> ok end,
 case erlang:monotonic_time(microsecond) >= Deadline of
  true -> $A = hd(I), Parent ! {done,K+1};
  false -> waits_loop(I,Deadline,K+1,Parent)
 end.
