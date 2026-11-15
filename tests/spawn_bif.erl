%% EXPECT: spawn_bif_ok
%% MFA spawn must run erlang BIFs in the child, as on BEAM.
%% In particular, invalid list_to_binary input must not raise in the caller.
-module(spawn_bif).
-export([test/0, wait_and_fail/2]).

test() ->
    Old = process_flag(trap_exit, true),
    try
        lists:foreach(fun(Kind) ->
            %% Good and bad arguments take the same child dispatch path.
            normal = reason(Kind, list_to_binary, [[1, <<2, 3>>]]),
            {badarg, BinaryStack} = reason(Kind, list_to_binary, [[1 | 2]]),
            true = is_list(BinaryStack),
            normal = reason(Kind, list_to_bitstring, [[<<1:3>>, <<2:5>>]]),
            {badarg, BitsStack} = reason(Kind, list_to_bitstring, [[256]]),
            true = is_list(BitsStack),

            %% Zero/two argument BIFs, and child-only process dictionary state.
            normal = reason(Kind, self, []),
            undefined = erase(spawn_bif_key),
            normal = reason(Kind, put, [spawn_bif_key, child_value]),
            undefined = get(spawn_bif_key),

            %% Exit-class reasons must not be replaced with error:badarg.
            ExitReason = {child_exit, lists:seq(1, 100)},
            ExitReason = reason(Kind, exit, [ExitReason]),

            %% A BIF may suspend or enter compiled code through apply/3.
            normal = reason(Kind, garbage_collect, []),
            ok = controlled(Kind)
        end, [spawn_link, spawn_monitor]),

        %% These previously omitted erlang BIF signatures must also be
        %% reachable through the child's shared apply dispatcher.
        wanted = reason(spawn_monitor, raise, [exit, wanted, []]),
        lists:foreach(fun({Function, Args}) ->
            normal = reason(spawn_monitor, Function, Args)
        end, [{is_bitstring, [<<5:3>>]},
              {term_to_binary, [{payload, [1, 2, 3]}]},
              {binary_to_term, [term_to_binary({payload, [1, 2, 3]})]},
              {bit_size, [<<5:3>>]},
              {spawn, [fun() -> ok end]},
              {monotonic_time, []},
              {monotonic_time, [millisecond]},
              {process_info, [self()]},
              {process_info, [self(), status]}]),

        %% Unlinked spawn also returns a PID for direct BIF success/failure.
        true = is_pid(spawn(erlang, list_to_binary, [[1, 2, 3]])),
        true = is_pid(spawn(erlang, list_to_binary, [[1 | 2]])),
        true = is_pid(erlang:spawn(erlang, list_to_binary, [[1 | 2]])),
        DynamicSpawn = fun erlang:spawn/3,
        true = is_pid(DynamicSpawn(erlang, list_to_binary, [[1 | 2]])),
        ok = controlled(spawn),

        %% A malformed outer Args list is still an error in the caller.
        lists:foreach(fun(Spawn) ->
            badarg = caller_error(Spawn, erlang, list_to_binary, not_a_list),
            badarg = caller_error(Spawn, erlang, list_to_binary, [[] | bad_tail]),
            badarg = caller_error(Spawn, not_an_atom(), list_to_binary, [[]]),
            badarg = caller_error(Spawn, erlang, not_an_atom(), [[]])
        end, [spawn, spawn_link, spawn_monitor]),
        io:format("spawn_bif_ok~n"),
        ok
    after
        process_flag(trap_exit, Old)
    end.

reason(spawn_link, Function, Args) ->
    Pid = spawn_link(erlang, Function, Args),
    true = is_pid(Pid),
    receive {'EXIT', Pid, Reason} -> Reason
    after 5000 -> error(missing_exit) end;
reason(spawn_monitor, Function, Args) ->
    {Pid, Ref} = spawn_monitor(erlang, Function, Args),
    true = is_pid(Pid),
    true = is_reference(Ref),
    down(Pid, Ref).

%% apply/3 is itself a BIF. Its compiled target waits so monitoring spawn/3
%% cannot race with process exit, and proves self() is the new process's PID.
controlled(Kind) ->
    Payload = {owned_reason, lists:seq(1, 1000), <<0:8192>>},
    {Pid, Ref} = start(Kind, [self(), Payload]),
    receive {ready, Pid} -> ok
    after 5000 -> error(missing_ready) end,
    true = garbage_collect(),
    Pid ! go,
    {Payload, Stack} = down(Pid, Ref),
    true = is_list(Stack),
    case Kind of
        spawn_link ->
            receive {'EXIT', Pid, {Payload, Stack}} -> ok
            after 5000 -> error(missing_linked_exit) end;
        _ -> ok
    end.

start(spawn_monitor, Args) ->
    spawn_monitor(erlang, apply, [?MODULE, wait_and_fail, Args]);
start(spawn_link, Args) ->
    Pid = spawn_link(erlang, apply, [?MODULE, wait_and_fail, Args]),
    {Pid, monitor(process, Pid)};
start(spawn, Args) ->
    Pid = spawn(erlang, apply, [?MODULE, wait_and_fail, Args]),
    {Pid, monitor(process, Pid)}.

wait_and_fail(Parent, Payload) ->
    Parent ! {ready, self()},
    receive go -> error(Payload) end.

down(Pid, Ref) ->
    receive {'DOWN', Ref, process, Pid, Reason} -> Reason
    after 5000 -> error(missing_down) end.

caller_error(Kind, Module, Function, Args) ->
    try
        case Kind of
            spawn -> spawn(Module, Function, Args);
            spawn_link -> spawn_link(Module, Function, Args);
            spawn_monitor -> spawn_monitor(Module, Function, Args)
        end
    of
        _ -> error(spawn_did_not_reject_args)
    catch error:badarg -> badarg end.

not_an_atom() -> 42.
