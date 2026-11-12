-module(hibernate_basic).
-export([test/0, wake_handler/1]).

test() ->
    Self = self(),
    Pid = spawn(fun() -> hibernate_loop(Self) end),
    %% Give the process time to enter hibernate
    timer:sleep(50),
    %% Send a message to wake it
    Pid ! {hello, Self},
    receive
        {reply, pong} ->
            ok
    after 5000 ->
        error(timeout)
    end.

hibernate_loop(Parent) ->
    erlang:hibernate(?MODULE, wake_handler, [Parent]).

wake_handler(Parent) ->
    receive
        {hello, From} ->
            From ! {reply, pong}
    after 0 ->
        ok
    end.
