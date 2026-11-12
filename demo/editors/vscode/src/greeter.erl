-module(greeter).
-include("demo.hrl").
-export([message/1, greet_all/1]).

-type greeting() :: {ok, string()}.
-type name() :: string().
-type greet_result(T) :: {ok, T} | {error, term()}.

-spec message(name()) -> greeting().
message(Name) when is_list(Name) ->
    State = #demo_state{},
    {ok, io_lib:format("~s says hello to ~s from ~p", [?APP_NAME, Name, State])}.

-spec greet_all([name()]) -> [greeting()].
greet_all(Names) ->
    [message(N) || N <- Names].
