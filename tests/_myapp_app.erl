-module(myapp_app).
-behaviour(application).

-export([start/2, stop/1]).

start(normal, _Args) ->
    myapp_sup:start_link().

stop(_Args) ->
    ok.
