%% Tests: receive inside receive, nested selective receive, timeout 0.
-module(nested_receive).
-export([test/0]).

test() ->
    %% Basic nested receive: outer waits for "start", inner waits for "data"
    Self = self(),
    Pid = spawn(fun() ->
        receive
            {start, From} ->
                From ! ready,
                receive
                    {data, Value} ->
                        From ! {result, Value * 2}
                end
        end
    end),
    Pid ! {start, Self},
    receive ready -> ok end,
    Pid ! {data, 21},
    receive {result, R1} -> 42 = R1 end,

    %% Timeout 0: non-blocking peek (empty mailbox)
    none = peek(),

    %% Timeout 0: non-blocking peek (message present)
    self() ! hello,
    {ok, hello} = peek(),

    %% Nested receive with different tags
    ok = nested_tags(),

    %% Receive inside a loop with inner receive
    ok = recv_loop_test(),

    io:format("ok~n"),
    ok.

peek() ->
    receive
        Msg -> {ok, Msg}
    after 0 ->
        none
    end.

nested_tags() ->
    Self = self(),
    spawn(fun() ->
        Self ! {a, 1},
        Self ! {b, 2},
        Self ! {a, 3}
    end),
    %% Small delay to let messages arrive
    receive after 50 -> ok end,
    %% Outer: get first {a, _}
    receive
        {a, V1} ->
            1 = V1,
            %% Inner: get {b, _} (skipping {a, 3})
            receive
                {b, V2} ->
                    2 = V2
            end
    end,
    %% The remaining {a, 3} should still be in mailbox
    receive
        {a, V3} -> 3 = V3
    after 100 ->
        error(missing_message)
    end,
    ok.

recv_loop_test() ->
    Self = self(),
    spawn(fun() ->
        Self ! {item, 1},
        Self ! {item, 2},
        Self ! {item, 3},
        Self ! done
    end),
    recv_loop([], Self).

recv_loop(Acc, _Self) ->
    receive
        {item, N} ->
            recv_loop([N | Acc], _Self);
        done ->
            [3, 2, 1] = Acc,
            ok
    after 1000 ->
        error(timeout)
    end.
