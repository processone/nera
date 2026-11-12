%% Tests idle heap compaction: GC runs when a process enters receive
%% with an oversized heap.  This verifies that the process can be GC'd
%% while entering receive-wait without corrupting its state.
-module(gc_receive_compact).
-export([test/0]).

test() ->
    %% Spawn a worker that allocates a lot of data, drops it,
    %% then enters receive and waits for a ping.
    Parent = self(),
    Pid = spawn(fun() ->
        %% Allocate a large list to grow the heap well past 256 KiB.
        %% Each element is a binary to also exercise binary vheap.
        BigList = make_big_list(5000, []),
        %% Use the list briefly so it's not optimized away
        Len = length(BigList),
        true = (Len =:= 5000),
        %% Drop all references — only the process dictionary
        %% and mailbox are roots now.
        Parent ! ready,
        %% This receive should trigger idle GC since heap > 256 KiB
        receive
            {ping, Val} ->
                %% After GC, the process should still function correctly
                Parent ! {pong, Val}
        end
    end),
    %% Wait for the worker to finish allocating and enter receive
    receive ready -> ok end,
    %% Small delay to let the process enter receive-wait
    timer:sleep(10),
    %% Send a message — the process should handle it correctly
    %% even though its heap was compacted on receive entry
    Pid ! {ping, 42},
    receive
        {pong, 42} -> ok
    after 2000 ->
        erlang:error(timeout)
    end,
    ok.

make_big_list(0, Acc) -> Acc;
make_big_list(N, Acc) ->
    %% Each binary is 200 bytes — total ~1 MB
    Bin = list_to_binary(lists:duplicate(200, N rem 256)),
    make_big_list(N - 1, [{N, Bin} | Acc]).
