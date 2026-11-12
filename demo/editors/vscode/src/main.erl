-module(main).
-export([start/0]).

-type exit_code() :: ok | error.

-spec start() -> exit_code().
start() ->
    {ok, Message} = greeter:message("Nera"),
    io:format("~s~n", [Message]),
    Results = greeter:greet_all(["Alice", "Bob"]),
    io:format("All: ~p~n", [Results]),
    ok.
