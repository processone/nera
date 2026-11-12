%% Adapted from "Programming Erlang" by Joe Armstrong — name server pattern.
%% Uses maps module (instead of the dict module from the book) to store
%% name-to-place mappings in a server process.
-module(jaerlang_name_server).
-export([test/0]).

id(I) -> I.

test() ->
    %% Start the name server
    Pid = spawn(fun() -> server_loop(maps:new()) end),
    register(name_server, Pid),

    %% Add some entries
    ok = rpc(name_server, {add, id(paris), id(france)}),
    ok = rpc(name_server, {add, id(london), id(england)}),
    ok = rpc(name_server, {add, id(tokyo), id(japan)}),

    %% Find existing entries
    {ok, france} = rpc(name_server, {find, id(paris)}),
    {ok, england} = rpc(name_server, {find, id(london)}),
    {ok, japan} = rpc(name_server, {find, id(tokyo)}),

    %% Find non-existing entry
    error = rpc(name_server, {find, id(berlin)}),

    %% Update an entry
    ok = rpc(name_server, {add, id(paris), id(france_updated)}),
    {ok, france_updated} = rpc(name_server, {find, id(paris)}),

    %% Get all names
    Names = rpc(name_server, all_names),
    3 = length(Names),
    true = lists:member(paris, Names),
    true = lists:member(london, Names),
    true = lists:member(tokyo, Names),

    %% Stop
    name_server ! stop,
    ok.

rpc(Server, Request) ->
    Pid = whereis(Server),
    Pid ! {self(), Request},
    receive
        {Pid, Response} ->
            Response
    end.

server_loop(Dict) ->
    receive
        {From, {add, Name, Place}} ->
            NewDict = maps:put(Name, Place, Dict),
            From ! {self(), ok},
            server_loop(NewDict);
        {From, {find, Name}} ->
            Result = maps:find(Name, Dict),
            From ! {self(), Result},
            server_loop(Dict);
        {From, all_names} ->
            From ! {self(), maps:keys(Dict)},
            server_loop(Dict);
        stop ->
            ok
    end.
