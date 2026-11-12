-module(initial_process_keepalive).
-export([test/0]).

%% EXPECT: keepalive_after_entry_return_ok
%% Regression: the startup process must retain its ETS tables and links
%% after the entry function returns. Nera keeps the startup caller alive;
%% BEAM needs an explicit caller loop to reproduce that lifecycle:
%%   erl -noshell -pa <beam-dir> -eval \
%%     'initial_process_keepalive:test(), receive after infinity -> ok end.'
%% Plain "-s initial_process_keepalive test" exits the startup process on
%% BEAM and deletes its ETS table. The child owns completion on both VMs.

test() ->
    ets:new(keepalive_test, [named_table, set, public]),
    ets:insert(keepalive_test, {counter, 0}),
    Parent = self(),
    Child = spawn_link(fun() -> check_after_return(Parent) end),
    %% No receive, sleep or other blocking operation follows this signal.
    %% Thus a subsequent waiting status means the startup caller has resumed
    %% after test/0 returned, not that test/0 is waiting for its child.
    Child ! entry_returning,
    ok.

check_after_return(Parent) ->
    try
        receive entry_returning -> ok
        after 5000 -> error(entry_signal_timeout)
        end,
        ok = await_entry_return(Parent, 5000),
        Parent = ets:info(keepalive_test, owner),
        {links, Links} = process_info(Parent, links),
        true = lists:member(self(), Links),
        1 = ets:update_counter(keepalive_test, counter, 1),
        [{counter, 1}] = ets:lookup(keepalive_test, counter),
        ets:delete(keepalive_test),
        io:format("keepalive_after_entry_return_ok~n"),
        halt(0)
    catch
        Class:Reason ->
            io:format("keepalive_after_entry_return_failed: ~p:~p~n", [Class, Reason]),
            halt(1)
    end.

await_entry_return(_Parent, 0) ->
    error(entry_return_timeout);
await_entry_return(Parent, Attempts) ->
    case process_info(Parent, status) of
        {status, waiting} -> ok;
        {status, Status} when Status =:= running; Status =:= runnable ->
            receive after 1 -> ok end,
            await_entry_return(Parent, Attempts - 1);
        Other -> error({unexpected_parent_status, Other})
    end.
