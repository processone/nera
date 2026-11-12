-module(monitor_exit_reason).
-export([test/0]).

id(I) -> I.

test() ->
    ok = test_error_reason(),
    ok = test_exit_reason(),
    ok = test_throw_reason(),
    ok = test_normal_exit(),
    ok = test_error_in_closure(),
    ok.

%% Test 1: error/1 — in BEAM, reason is {boom, Stacktrace}
%% At minimum it must NOT be 'normal'
test_error_reason() ->
    {Pid, Ref} = spawn_monitor(fun() -> error(id(boom)) end),
    receive
        {'DOWN', Ref, process, Pid, Reason} ->
            case Reason of
                normal -> erlang:error({wrong_reason, expected_not_normal, got, normal});
                _ -> ok
            end
    after 2000 ->
        erlang:error(timeout_error_reason)
    end.

%% Test 2: exit/1 with explicit reason
test_exit_reason() ->
    {Pid, Ref} = spawn_monitor(fun() -> exit(id(byebye)) end),
    receive
        {'DOWN', Ref, process, Pid, Reason} ->
            case Reason of
                byebye -> ok;
                Other -> erlang:error({wrong_exit_reason, expected, byebye, got, Other})
            end
    after 2000 ->
        erlang:error(timeout_exit_reason)
    end.

%% Test 3: throw/1 with no catch — reason should NOT be normal
test_throw_reason() ->
    {Pid, Ref} = spawn_monitor(fun() -> throw(id(oops)) end),
    receive
        {'DOWN', Ref, process, Pid, Reason} ->
            case Reason of
                normal -> erlang:error({wrong_throw_reason, expected_not_normal, got, normal});
                _ -> ok
            end
    after 2000 ->
        erlang:error(timeout_throw_reason)
    end.

%% Test 4: normal exit should produce 'normal'
test_normal_exit() ->
    {Pid, Ref} = spawn_monitor(fun() -> id(done) end),
    receive
        {'DOWN', Ref, process, Pid, Reason} ->
            case Reason of
                normal -> ok;
                Other -> erlang:error({wrong_normal_reason, expected, normal, got, Other})
            end
    after 2000 ->
        erlang:error(timeout_normal_exit)
    end.

%% Test 5: error/1 deep inside a closure chain
%% This is the scenario from Bug #8 — closures may wrap error exits differently
test_error_in_closure() ->
    F = fun() ->
        G = fun() -> error(id(deep_boom)) end,
        G()
    end,
    {Pid, Ref} = spawn_monitor(F),
    receive
        {'DOWN', Ref, process, Pid, Reason} ->
            case Reason of
                normal -> erlang:error({wrong_closure_reason, expected_not_normal, got, normal});
                _ -> ok
            end
    after 2000 ->
        erlang:error(timeout_closure_error)
    end.
