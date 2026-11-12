%% Adapted from "Programming Erlang, Second Edition" by Joe Armstrong
%% (server1.erl + name_server.erl) — a simple RPC server framework.
%%
%% Pattern: A registered server process handles {From, Request} messages
%% and replies with {Name, Response}. State is maintained across calls.
%% The original uses dict; this version uses maps.
%% The original uses dynamic Mod:handle() callbacks; this version inlines
%% the callback since nera doesn't yet support dynamic remote calls.
-module(jaerlang_server).
-export([test/0]).

%% --- server (adapted from server1.erl, callback inlined) ---

start() ->
    register(name_server, spawn(fun() -> loop(#{}) end)).

rpc(Request) ->
    name_server ! {self(), Request},
    receive
        {name_server, Response} -> Response
    end.

loop(State) ->
    receive
        {From, stop} ->
            From ! {name_server, stopped},
            ok;
        {From, Request} ->
            {Response, State1} = handle(Request, State),
            From ! {name_server, Response},
            loop(State1)
    end.

%% --- callback (adapted from name_server.erl, using maps) ---

handle({add, Name, Place}, Map) ->
    {ok, Map#{Name => Place}};
handle({find, Name}, Map) ->
    case maps:find(Name, Map) of
        {ok, Value} -> {{ok, Value}, Map};
        error       -> {error, Map}
    end.

%% --- client helpers ---

add(Name, Place) -> rpc({add, Name, Place}).
find(Name)       -> rpc({find, Name}).

%% --- test ---

test() ->
    %% Start the name server
    start(),

    %% Verify it's registered
    true = is_pid(whereis(name_server)),

    %% Add some entries
    ok = add(joe,    stockholm),
    ok = add(robert, paris),
    ok = add(mike,   london),

    %% Look them up
    {ok, stockholm} = find(joe),
    {ok, paris}     = find(robert),
    {ok, london}    = find(mike),

    %% Missing key
    error = find(unknown),

    %% Overwrite an entry
    ok = add(joe, gothenburg),
    {ok, gothenburg} = find(joe),

    %% Stop the server cleanly
    stopped = rpc(stop),
    timer_sleep(100),

    %% Verify auto-cleanup of the registered name
    undefined = whereis(name_server),

    io:format("ok~n"),
    ok.

timer_sleep(Ms) ->
    receive after Ms -> ok end.

id(I) -> I.
