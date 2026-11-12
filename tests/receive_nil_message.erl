%% EXPECT: receive_nil_message_ok
%% Regression: a process receives the message [] like any other message, and
%% the messages queued after it are still received, as on BEAM. Before the
%% fix, the mailbox scan answered [] to mean "no more messages", so a []
%% message looked like an empty mailbox: receive timed out on it and on every
%% message behind it. Run this same fixture on native Nera and BEAM.
-module(receive_nil_message).
-export([test/0]).

test() ->
    %% The reported sequence: send a, [], b and receive after each send.
    self() ! a, {got, a} = r(),
    self() ! id([]), {got, []} = r(),
    self() ! b, {got, b} = r(),
    timeout = r(),

    %% [] first, in the middle and last, all received in order.
    send_all([[], x, [], y, []]),
    [[], x, [], y, []] = drain(),

    %% Only [] messages, received one by one, then an empty mailbox.
    send_all([[], [], []]),
    [[], [], []] = drain(),

    %% A literal [] pattern and a guard both match the [] message.
    self() ! id([]),
    nil = receive [] -> nil after 100 -> timeout end,
    self() ! id([]),
    empty_list = receive L when is_list(L), length(L) =:= 0 -> empty_list
                 after 100 -> timeout end,

    %% after 0, and a receive with no after clause.
    self() ! id([]),
    {got, []} = receive X0 -> {got, X0} after 0 -> timeout end,
    self() ! id([]),
    [] = receive X1 -> X1 end,

    %% Selective receive skips a [] it does not match, reaches the messages
    %% behind it, and leaves [] in the mailbox in its place.
    send_all([[], c, [], d]),
    d = receive d -> d after 100 -> timeout end,
    c = receive c -> c after 100 -> timeout end,
    [[], []] = drain(),

    %% No clause matches []: receive times out and keeps it.
    self() ! id([]),
    timeout = receive {never, _} -> never after 0 -> timeout end,
    timeout = receive after 10 -> timeout end,
    [[]] = drain(),

    %% [] sent by another process while the receiver waits, then a message
    %% the receiver's clause does match, queued behind the skipped [].
    Parent = self(),
    Waiter = spawn(fun() -> waiter(Parent) end),
    ready = receive {Waiter, ready} -> ready after 1000 -> timeout end,
    pause(),
    Waiter ! id([]),
    pause(),
    Waiter ! {tag, 1},
    {Waiter, {tagged, 1}, [[]]} =
        receive {Waiter, _, _} = Got -> Got after 1000 -> timeout end,

    %% A waiting receiver that matches anything gets [] first, then later ones.
    Echo = spawn(fun() -> echo(Parent) end),
    ready = receive {Echo, ready} -> ready after 1000 -> timeout end,
    pause(),
    Echo ! id([]),
    pause(),
    Echo ! e,
    Echo ! id([]),
    [[], e, []] = [receive {Echo, M} -> M after 1000 -> timeout end
                   || _ <- [1, 2, 3]],

    timeout = r(),
    io:format("receive_nil_message_ok~n"),
    ok.

r() -> receive X -> {got, X} after 100 -> timeout end.

waiter(Parent) ->
    Parent ! {self(), ready},
    Got = receive {tag, N} -> {tagged, N} after 1000 -> timeout end,
    Parent ! {self(), Got, drain()}.

%% Forward three messages to Parent, each tagged with this process.
echo(Parent) ->
    Parent ! {self(), ready},
    echo_loop(Parent, 3).

echo_loop(_Parent, 0) -> ok;
echo_loop(Parent, N) ->
    M = receive X -> X after 1000 -> timeout end,
    Parent ! {self(), M},
    echo_loop(Parent, N - 1).

%% Receive every queued message, in order, until the mailbox is empty.
drain() ->
    receive M -> [M | drain()] after 0 -> [] end.

send_all(Messages) -> lists:foreach(fun(M) -> self() ! M end, Messages).

pause() -> receive after 20 -> ok end.

id(X) -> X.
