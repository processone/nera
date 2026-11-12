-module(guide_hello).
-export([start/0]).

start() ->
    Numbers = [1, 2, 3, 4],
    Squares = [N * N || N <- Numbers],
    io:format("Hello from native Erlang!~n"),
    io:format("Squares: ~p~n", [Squares]).
