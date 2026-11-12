-module(process_info).
-export([test/0]).

%% Tests for process_info/1 and process_info/2.

id(I) -> I.

test() ->
    %% ---- Single-item /2 form ----

    %% message_queue_len, empty mailbox
    {message_queue_len, 0} = process_info(self(), message_queue_len),

    %% message_queue_len, after sending messages to self
    self() ! id(a),
    self() ! id(b),
    self() ! id(c),
    {message_queue_len, 3} = process_info(self(), message_queue_len),

    %% messages item: list of terms in the mailbox
    {messages, [a, b, c]} = process_info(self(), messages),

    %% Drain the mailbox
    a = receive M1 -> M1 after 1000 -> timeout end,
    b = receive M2 -> M2 after 1000 -> timeout end,
    c = receive M3 -> M3 after 1000 -> timeout end,
    {message_queue_len, 0} = process_info(self(), message_queue_len),
    {messages, []} = process_info(self(), messages),

    %% Boolean + atom-valued items
    {trap_exit, false} = process_info(self(), trap_exit),
    {status, running} = process_info(self(), status),

    %% Integer-valued items: value varies, just check the shape
    {reductions, Reds} = process_info(self(), reductions),
    true = id(is_integer(Reds)),
    {heap_size, HS} = process_info(self(), heap_size),
    true = id(is_integer(HS)),
    {memory, Mem} = process_info(self(), memory),
    true = id(is_integer(Mem)),

    %% Lists of pids
    {links, Links} = process_info(self(), links),
    true = id(is_list(Links)),
    {monitors, Monitors} = process_info(self(), monitors),
    true = id(is_list(Monitors)),
    {monitored_by, MonBy} = process_info(self(), monitored_by),
    true = id(is_list(MonBy)),

    %% Dictionary
    undefined = put(foo, 42),
    {dictionary, Dict} = process_info(self(), dictionary),
    true = id(is_list(Dict)),
    {foo, 42} = id(lists:keyfind(foo, 1, Dict)),
    erase(foo),

    %% Stub items still return sensible shapes
    {group_leader, GL} = process_info(self(), group_leader),
    true = id(is_pid(GL)),
    {priority, normal} = process_info(self(), priority),

    %% registered_name on an unnamed process: [] per OTP
    {registered_name, []} = process_info(self(), registered_name),

    %% ---- List-arg /2 form ----

    L1 = process_info(self(), [message_queue_len, status, trap_exit]),
    [{message_queue_len, 0}, {status, running}, {trap_exit, false}] = L1,

    %% Empty item list → empty result
    [] = process_info(self(), []),

    %% ---- /1 form (all items) ----

    Info = process_info(self()),
    true = id(is_list(Info)),
    {message_queue_len, 0} = id(lists:keyfind(message_queue_len, 1, Info)),
    {status, running} = id(lists:keyfind(status, 1, Info)),
    {trap_exit, false} = id(lists:keyfind(trap_exit, 1, Info)),
    %% registered_name is omitted from /1 when unset
    false = id(lists:keyfind(registered_name, 1, Info)),

    %% ---- Dead process returns undefined ----

    DeadPid = spawn(fun() -> ok end),
    %% Wait for child to exit
    receive after 100 -> ok end,
    undefined = process_info(DeadPid, message_queue_len),
    undefined = process_info(DeadPid),

    %% ---- Unknown item raises badarg ----

    try process_info(self(), totally_bogus_item) of
        _ -> erlang:error(expected_badarg)
    catch
        error:badarg -> ok
    end,

    %% ---- Query another live process ----

    Parent = self(),
    Pid = spawn(fun() ->
        Parent ! id(ready),
        receive
            check -> Parent ! id(checked)
        after 5000 -> ok
        end
    end),
    ready = receive R -> R after 5000 -> timeout end,
    %% Target is alive but has nothing queued (consumed by worker loop)
    {message_queue_len, _} = process_info(Pid, message_queue_len),
    %% Can query info items on another process
    {links, _} = process_info(Pid, links),
    Pid ! id(check),
    receive after 50 -> ok end,
    checked = receive R2 -> R2 after 5000 -> timeout end,

    io:format("ok~n"),
    ok.
