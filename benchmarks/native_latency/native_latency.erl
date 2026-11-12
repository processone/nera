-module(native_latency).
-export([small/0, large/0, idle/0, decode_large/0, decode_spaces/0, decode_error/0]).
small() -> run(49149, 2048).
large() -> run(8*1024*1024, 16).
idle() -> run(0, 0).
decode_large() -> run(8*1024*1024, 16, decode).
decode_spaces() -> run(8*1024*1024, 16, spaces).
decode_error() -> run(8*1024*1024, 16, late_error).
run(Size, Count) -> run(Size, Count, encode).
run(Size, Count, Kind) ->
    Input = case Kind of
        encode -> binary:copy(<<0>>, Size);
        decode -> binary:copy(<<"A">>, Size);
        spaces -> binary:copy(<<" ">>, Size);
        late_error -> Space = binary:copy(<<" ">>, Size), <<Space/binary,"Y===">>
    end,
    Parent = self(),
    Probe = spawn(fun() -> probe() end),
    Worker = spawn(fun() -> receive go -> perform(Input, Count, Kind), Parent ! done end end),
    Start = erlang:monotonic_time(microsecond),
    Worker ! go,
    Minimum = case Count of 0 -> 2000; _ -> 1 end,
    collect(Probe, Start, Start, [], false, 0, Minimum).
perform(_, 0, _) -> ok;
perform(Input, N, Kind) ->
    case Kind of
        encode -> R = base64:encode(Input, #{}),
                  true = byte_size(R) =:= ((byte_size(Input)+2) div 3)*4;
        decode -> R = base64:decode(Input, #{}),
                  true = byte_size(R) =:= (byte_size(Input) div 4)*3;
        spaces -> <<>> = base64:decode(Input, #{});
        late_error -> try base64:decode(Input, #{}) of
                          _ -> error(expected_bad_decode)
                      catch error:badarith -> ok end
    end,
    perform(Input, N-1, Kind).
probe() -> receive
    {ping, P} -> P ! pong, probe();
    stop -> ok
end.
collect(Probe, Start, _, Samples, true, N, Minimum) when N >= Minimum ->
    Probe ! stop,
    io:format("elapsed ~p~n", [erlang:monotonic_time(microsecond)-Start]),
    lists:foreach(fun(X) -> io:format("~p~n", [X]) end, Samples);
collect(Probe, Start, _, Samples, Done, N, Minimum) ->
    T = erlang:monotonic_time(microsecond),
    Probe ! {ping, self()},
    wait(Probe, Start, T, Samples, Done, N, Minimum).
wait(Probe, Start, T, Samples, Done, N, Minimum) -> receive
    done -> wait(Probe, Start, T, Samples, true, N, Minimum);
    pong ->
        Delta = erlang:monotonic_time(microsecond)-T,
        collect(Probe, Start, T, [Delta|Samples], Done, N+1, Minimum)
end.
