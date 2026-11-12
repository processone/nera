-module(main).
-export([start/0]).

%% Test that multiple processes can do blocking I/O concurrently
%% without deadlocking the scheduler, and that computation processes
%% keep running while I/O is in flight.

start() ->
    test(),
    ok.

test() ->
    Self = self(),
    N = 6,

    %% Spawn N processes each doing file write + read
    [spawn(fun() ->
        Fname = "/tmp/nera_io_concurrent_" ++ integer_to_list(I),
        Data = <<"concurrent_data_", (I + 48):8>>,
        ok = file:write_file(Fname, Data),
        {ok, ReadBack} = file:read_file(Fname),
        Data = ReadBack,
        Self ! {done_io, I}
    end) || I <- lists:seq(1, N)],

    %% Spawn a computation process that runs concurrently with I/O
    spawn(fun() ->
        _ = fib(25),
        Self ! {done_compute, 0}
    end),

    %% Wait for all N I/O processes + 1 compute process
    wait(N + 1),

    %% Cleanup temp files
    [file:write_file("/tmp/nera_io_concurrent_" ++ integer_to_list(I), <<>>) || I <- lists:seq(1, N)],

    ok.

wait(0) -> ok;
wait(N) ->
    receive
        {done_io, _} -> wait(N - 1);
        {done_compute, _} -> wait(N - 1)
    after 5000 ->
        exit(timeout)
    end.

fib(0) -> 0;
fib(1) -> 1;
fib(N) -> fib(N - 1) + fib(N - 2).
