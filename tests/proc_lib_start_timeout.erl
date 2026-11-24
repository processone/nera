%% EXPECT: proc_lib_start_timeout_ok
%% Intent: finite proc_lib:start_link/5 timeouts must stop an unacknowledged
%% child and return {error, timeout}, as BEAM does, without killing the caller.
-module(proc_lib_start_timeout).
-export([test/0, init/2]).

test() ->
    lists:foreach(fun(Trap) ->
        in_caller(Trap, fun() ->
            self() ! unrelated,
            OtherRef = make_ref(),
            self() ! {'DOWN', OtherRef, process, self(), unrelated},
            self() ! {'EXIT', self(), unrelated},
            {error, timeout} = proc_lib:start_link(?MODULE, init, [self(), wait], 250, []),
            receive {child, Child} ->
                false = is_process_alive(Child)
            after 5000 -> error(child_never_started)
            end,
            receive unrelated -> ok after 0 -> error(lost_message) end,
            receive {'DOWN', OtherRef, process, _, unrelated} -> ok
            after 0 -> error(lost_unrelated_down)
            end,
            receive {'EXIT', _, unrelated} -> ok
            after 0 -> error(lost_unrelated_exit)
            end,
            no_messages(),
            io:format("timeout trap=~p: returned error, child dead, mailbox preserved~n", [Trap])
        end),
        in_caller(Trap, fun() ->
            {error, timeout} = proc_lib:start_link(?MODULE, init, [self(), silent], 0, []),
            no_messages(),
            io:format("zero timeout trap=~p: caller survived, no startup signals~n", [Trap])
        end)
    end, [true, false]),
    lists:foreach(fun(Timeout) ->
        in_caller(true, fun() ->
            {ok, Child} = proc_lib:start_link(?MODULE, init, [self(), ack], Timeout, []),
            true = is_process_alive(Child),
            Child ! stop,
            receive {'EXIT', Child, normal} -> ok
            after 5000 -> error(link_not_preserved)
            end,
            no_messages(),
            io:format("ack timeout=~p: returned ok, link preserved~n", [Timeout])
        end)
    end, [5000, infinity]),
    in_caller(true, fun() ->
        {error, before_ack} = proc_lib:start_link(?MODULE, init, [self(), crash], 5000, []),
        no_messages()
    end),
    io:format("proc_lib_start_timeout_ok~n"),
    ok.

init(Parent, wait) ->
    process_flag(trap_exit, true),
    Parent ! {child, self()},
    receive never -> ok end;
init(_Parent, silent) ->
    receive never -> ok end;
init(_Parent, crash) -> exit(before_ack);
init(_Parent, ack) ->
    proc_lib:init_ack({ok, self()}),
    receive stop -> ok end.

in_caller(Trap, Fun) ->
    Parent = self(),
    {Pid, Ref} = spawn_monitor(fun() ->
        process_flag(trap_exit, Trap),
        Fun(),
        Parent ! {completed, self()}
    end),
    receive
        {'DOWN', Ref, process, Pid, Reason} ->
            normal = Reason,
            receive {completed, Pid} -> ok
            after 0 -> error(caller_did_not_complete)
            end
    after 10000 ->
        exit(Pid, kill),
        error(start_link_still_waiting)
    end.

no_messages() ->
    receive Msg -> error({unexpected_message, Msg})
    after 0 -> ok
    end.
