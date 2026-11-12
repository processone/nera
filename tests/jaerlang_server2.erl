%% Adapted from "Programming Erlang, Second Edition" by Joe Armstrong
%% (server2.erl) — RPC server with try/catch error recovery.
%%
%% Pattern: Like server1, but wraps the handler in try...of...catch.
%% On success the state advances; on failure the server logs the error,
%% sends a crash notification, and continues with the OLD state.
%% This demonstrates fault-tolerant server patterns with registered names.
-module(jaerlang_server2).
-export([test/0]).

%% --- server (adapted from server2.erl, callback inlined) ---

start() ->
    register(kv_server, spawn(fun() -> loop(#{}) end)).

rpc(Request) ->
    kv_server ! {self(), Request},
    receive
        {kv_server, crash}       -> {error, crashed};
        {kv_server, ok, Response} -> Response
    end.

loop(OldState) ->
    receive
        {From, stop} ->
            From ! {kv_server, ok, stopped},
            ok;
        {From, Request} ->
            try handle(Request, OldState) of
                {Response, NewState} ->
                    From ! {kv_server, ok, Response},
                    loop(NewState)
            catch
                _:_Why ->
                    From ! {kv_server, crash},
                    %% Loop with the ORIGINAL state (error recovery)
                    loop(OldState)
            end
    end.

%% --- handler callbacks ---

handle({put, Key, Value}, Map) ->
    {ok, Map#{Key => Value}};
handle({get, Key}, Map) ->
    case maps:find(Key, Map) of
        {ok, Val} -> {{ok, Val}, Map};
        error     -> {error, Map}
    end;
handle({delete, Key}, Map) ->
    {ok, maps:remove(Key, Map)};
handle(crash_now, _Map) ->
    %% Intentionally crash to test error recovery
    error(deliberate_crash).

%% --- client helpers ---

put_val(Key, Value) -> rpc({put, Key, Value}).
get_val(Key)        -> rpc({get, Key}).
delete_val(Key)     -> rpc({delete, Key}).
crash_it()          -> rpc(crash_now).

%% --- test ---

test() ->
    start(),
    true = is_pid(whereis(kv_server)),

    %% Basic put/get
    ok = put_val(name, alice),
    ok = put_val(age, 30),
    {ok, alice} = get_val(name),
    {ok, 30}    = get_val(age),
    error       = get_val(missing),

    %% Delete
    ok = put_val(temp, 999),
    {ok, 999} = get_val(temp),
    ok = delete_val(temp),
    error = get_val(temp),

    %% Crash recovery: the handler crashes, but the server survives
    %% with its old state intact
    {error, crashed} = crash_it(),

    %% Server is still alive and state is preserved
    {ok, alice} = get_val(name),
    {ok, 30}    = get_val(age),

    %% Can still mutate state after recovery
    ok = put_val(city, berlin),
    {ok, berlin} = get_val(city),

    %% Clean shutdown via stop message
    stopped = rpc(stop),
    timer_sleep(100),
    undefined = whereis(kv_server),

    io:format("ok~n"),
    ok.

timer_sleep(Ms) ->
    receive after Ms -> ok end.

id(I) -> I.
