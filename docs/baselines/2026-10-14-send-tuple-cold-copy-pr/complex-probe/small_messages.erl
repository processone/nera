-module(small_messages).
-export([atom/0, tuple2/0, tuple3/0, tuple4/0,
         atom_multi/0, tuple2_multi/0, tuple3_multi/0, tuple4_multi/0]).
atom()->run(atom,100).
tuple2()->run(tuple2,100).
tuple3()->run(tuple3,100).
tuple4()->run(tuple4,100).
atom_multi()->run(atom,30).
tuple2_multi()->run(tuple2,30).
tuple3_multi()->run(tuple3,30).
tuple4_multi()->run(tuple4,30).
run(Kind,Batches)->lists:foreach(fun(_)->batch(Kind,20000) end,lists:seq(1,Batches)).
batch(Kind,Count)->
    Parent=self(),
    Receiver=spawn(fun()->consume(Parent,Count,0) end),
    Worker=spawn(fun()->receive go->send_loop(Kind,Receiver,Count) end end),
    Probe=spawn(fun echo/0),
    Start=erlang:monotonic_time(microsecond),
    Worker!go,
    Expected=case Kind of atom->Count;_->Count*(Count+1) div 2 end,
    collect(Probe,Receiver,Expected,Start,[]).
send_loop(_,_,0)->ok;
send_loop(atom,P,N)->P!ping,send_loop(atom,P,N-1);
send_loop(tuple2,P,N)->P!{small,[N]},send_loop(tuple2,P,N-1);
send_loop(tuple3,P,N)->P!{small,N,ok},send_loop(tuple3,P,N-1);
send_loop(tuple4,P,N)->P!{small,N,ok,done},send_loop(tuple4,P,N-1).
consume(Parent,0,Sum)->Parent!{done,self(),Sum};
consume(Parent,N,Sum)->receive
    ping->consume(Parent,N-1,Sum+1);
    {small,[V]}->consume(Parent,N-1,Sum+V);
    {small,V,ok}->consume(Parent,N-1,Sum+V);
    {small,V,ok,done}->consume(Parent,N-1,Sum+V)
end.
echo()->receive
    {ping,P}->P!{pong,self()},echo();
    {stop,P}->P!{stopped,self()}
end.
collect(Probe,Receiver,Expected,Start,Samples)->
    T=erlang:monotonic_time(microsecond),
    Probe!{ping,self()},
    receive
        {pong,Probe}->collect(Probe,Receiver,Expected,Start,
                             [erlang:monotonic_time(microsecond)-T|Samples]);
        {done,Receiver,Actual}->
            Elapsed=erlang:monotonic_time(microsecond)-Start,
            Expected=Actual,
            %% One ping is outstanding. Drain it before stopping the probe,
            %% so no stale pong can contaminate the next batch's timing.
            FinalRTT=receive {pong,Probe}->erlang:monotonic_time(microsecond)-T
                     after 10000->error(probe_timeout) end,
            Probe!{stop,self()},
            receive {stopped,Probe}->ok after 10000->error(stop_timeout) end,
            io:format("batch 20000 ~p~n~w~n",[Elapsed,[FinalRTT|Samples]])
    after 10000->error(batch_timeout)
    end.
