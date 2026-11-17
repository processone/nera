%% EXPECT: send_name_node_ok
%% `{Name, Node} ! Msg` with Node the local node, as on BEAM: a registered
%% name receives Msg, a name nobody registered drops it without an error,
%% and the send evaluates to Msg in both cases, inside and outside try.
%% A plain message and a tuple built in the send take different native
%% paths; both are covered. Any other tuple destination is still badarg.
%% Run this same fixture on native Nera and BEAM.
-module(send_name_node).
-export([test/0]).

test() ->
    Node = node(),
    ok = to_other_process(Node),
    ok = to_self(Node),
    ok = unregistered(Node),
    ok = unregistered_in_try(Node),
    ok = other_node(Node),
    ok = invalid(Node),
    io:format("send_name_node_ok~n").

%% A process registered under a name gets each message once and in order.
%% Once it has exited, its name is free and a send to it is dropped.
to_other_process(Node) ->
    Parent = self(),
    {Pid, Ref} = spawn_monitor(fun() ->
        true = register(send_name_node_peer, self()),
        Parent ! {self(), registered},
        echo(Parent)
    end),
    receive {Pid, registered} -> ok end,
    Dest = id({send_name_node_peer, Node}),
    plain = Dest ! plain,
    {tuple, 1, <<"b">>} = Dest ! {tuple, 1, <<"b">>},
    Bound = id({bound, [1, 2]}),
    Bound = Dest ! Bound,
    stop = Dest ! stop,
    receive {'DOWN', Ref, process, Pid, normal} -> ok end,
    [{Pid, plain}, {Pid, {tuple, 1, <<"b">>}}, {Pid, {bound, [1, 2]}}] = flush(),
    late = Dest ! late,
    {late, 2} = Dest ! {late, 2},
    [] = flush(),
    ok.

echo(Parent) ->
    receive
        stop -> ok;
        Msg -> Parent ! {self(), Msg}, echo(Parent)
    end.

to_self(Node) ->
    true = register(send_name_node_self, self()),
    Dest = id({send_name_node_self, Node}),
    first = Dest ! first,
    {second, 2} = Dest ! {second, 2},
    true = unregister(send_name_node_self),
    [first, {second, 2}] = flush(),
    ok.

%% The process goes on after the send: nothing stops it at the send.
unregistered(Node) ->
    undefined = whereis(send_name_node_nobody),
    Dest = id({send_name_node_nobody, Node}),
    dropped = Dest ! dropped,
    {dropped, 2} = Dest ! {dropped, 2},
    self() ! after_drop,
    [after_drop] = flush(),
    ok.

%% The send does not leave the try body: the rest of it runs.
unregistered_in_try(Node) ->
    ok = try
             _ = {id(send_name_node_nobody), node()} ! x,
             self() ! sent,
             ok
         catch C:E -> {C, E}
         end,
    ok = try
             _ = id({send_name_node_nobody, Node}) ! {x, 1},
             self() ! {sent, 2},
             ok
         catch C2:E2 -> {C2, E2}
         end,
    [sent, {sent, 2}] = flush(),
    ok.

%% A node that is not alive drops a send to any other node, even to a name
%% registered here; Nera is never distributed. A distributed BEAM node would
%% try to reach that node instead, so the case is skipped there.
other_node(nonode@nohost) ->
    true = register(send_name_node_here, self()),
    Dest = id({send_name_node_here, 'send_name_node@elsewhere'}),
    remote = Dest ! remote,
    {remote, 2} = Dest ! {remote, 2},
    true = unregister(send_name_node_here),
    [] = flush(),
    ok;
other_node(_) ->
    ok.

%% Only {atom, atom} names a process this way; a bare name must still be
%% registered.
invalid(Node) ->
    true = register(send_name_node_self, self()),
    Dests = [{"send_name_node_self", Node}, {send_name_node_self, "node"},
             {send_name_node_self, Node, x}, {self(), Node},
             {send_name_node_self}, send_name_node_nobody],
    lists:foreach(fun(Dest) ->
        {Dest, {error, badarg}} =
            {Dest, try id(Dest) ! x catch C:E -> {C, E} end},
        {Dest, {error, badarg}} =
            {Dest, try id(Dest) ! {x, 1} catch C2:E2 -> {C2, E2} end}
    end, Dests),
    true = unregister(send_name_node_self),
    [] = flush(),
    ok.

flush() ->
    receive Msg -> [Msg | flush()] after 0 -> [] end.

id(X) -> X.
