%% Application callback module for application_start_ignore: its start/2
%% returns the result of starting a supervisor whose init/1 returns ignore.
-module(ignored_app).
-export([start/2, stop/1, init/1]).

start(_Type, _Args) ->
    supervisor:start_link(?MODULE, []).

stop(_State) ->
    ok.

init([]) ->
    ignore.
