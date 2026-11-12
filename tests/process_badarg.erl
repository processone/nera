-module(process_badarg).
-export([test/0]).

%% Test that spawn, spawn_link, spawn_closure, send, ets:new,
%% and erlang:monotonic_time/1 raise error:badarg on invalid args.

test() ->
    %% spawn(not_a_fun) — non-closure argument
    caught = try_badarg(fun() -> spawn(id(123)) end),
    %% spawn(not_a_fun)
    caught = try_badarg(fun() -> spawn(id(not_a_fun)) end),
    %% spawn_link(not_a_fun)
    caught = try_badarg(fun() -> spawn_link(id(not_a_fun)) end),
    %% spawn_link with integer
    caught = try_badarg(fun() -> spawn_link(id(123)) end),
    %% send to non-pid/non-atom
    caught = try_badarg(fun() -> id(123) ! hello end),
    %% send to unregistered name
    caught = try_badarg(fun() -> id(nonexistent_name_xyz) ! hello end),
    %% ets:new with non-atom name
    caught = try_badarg(fun() -> ets:new(id(123), []) end),
    %% monotonic_time with non-atom unit
    caught = try_badarg(fun() -> erlang:monotonic_time(id(123)) end),
    %% monotonic_time with unknown atom unit
    caught = try_badarg(fun() -> erlang:monotonic_time(id(bogus_unit)) end),
    %% Normal cases still work
    Pid = spawn(fun() -> ok end),
    true = is_pid(Pid),
    T = erlang:monotonic_time(millisecond),
    true = is_integer(T),
    ok.

try_badarg(F) ->
    try F()
    catch error:badarg -> caught
    end.

id(X) -> X.
