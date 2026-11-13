%% EXPECT: send_order_between_processes_ok
%% Regression: the messages one process sends another arrive in the order
%% they were sent, as on BEAM. Before the fix, a sender could append a
%% message straight to the receiver's mailbox while its earlier message still
%% waited in the receiver's queue of incoming messages, so the later message
%% was received first. receive_nil_message failed this way now and then:
%% [[], [], e] instead of [[], e, []]. Run this same fixture on native Nera
%% and BEAM.
-module(send_order_between_processes).
-export([test/0]).

-define(PAIRS, 4).
-define(MESSAGES, 100000).

test() ->
    %% Each sender streams 1..?MESSAGES to its own receiver while the others
    %% run, and each receiver counts the numbers that do not follow the one
    %% before. The senders run while their receivers are taking messages,
    %% waiting for the next one or being woken, so every delivery path is
    %% exercised.
    Main = self(),
    Receivers = [spawn(fun() -> receiver(Main, 0, 0) end)
                 || _ <- lists:seq(1, ?PAIRS)],
    [spawn(fun() -> sender(Receiver, 1) end) || Receiver <- Receivers],
    Results = [receive {Receiver, Last, OutOfOrder} -> {Last, OutOfOrder}
               after 20000 -> timeout end
               || Receiver <- Receivers],
    Expected = lists:duplicate(?PAIRS, {?MESSAGES, 0}),
    Expected = Results,

    %% The pattern receive_nil_message checks: a burst sent to an echo
    %% process comes back in the order it was sent, round after round.
    Echo = spawn(fun() -> echo(Main) end),
    ok = bursts(Echo, 2000),
    Echo ! stop,

    io:format("send_order_between_processes_ok~n"),
    ok.

sender(_Receiver, N) when N > ?MESSAGES -> ok;
sender(Receiver, N) ->
    Receiver ! N,
    sender(Receiver, N + 1).

%% Stop at the last number, whenever it arrives, and report how many
%% numbers did not follow the previous one.
receiver(Main, Last, OutOfOrder) ->
    receive
        N when N =:= ?MESSAGES ->
            Main ! {self(), N, count(N, Last, OutOfOrder)};
        N ->
            receiver(Main, N, count(N, Last, OutOfOrder))
    end.

count(N, Last, OutOfOrder) when N =:= Last + 1 -> OutOfOrder;
count(_N, _Last, OutOfOrder) -> OutOfOrder + 1.

bursts(_Echo, 0) -> ok;
bursts(Echo, Round) ->
    Echo ! {Round, 1},
    Echo ! {Round, 2},
    Echo ! {Round, 3},
    [{Round, 1}, {Round, 2}, {Round, 3}] =
        [receive {Echo, M} -> M after 1000 -> timeout end || _ <- [1, 2, 3]],
    bursts(Echo, Round - 1).

echo(Parent) ->
    receive
        stop -> ok;
        M -> Parent ! {self(), M}, echo(Parent)
    end.
