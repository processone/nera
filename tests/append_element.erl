-module(append_element).
-export([test/0]).

test() ->
    %% Basic append
    {a, b, c} = erlang:append_element({a, b}, c),
    %% Append to single-element tuple
    {x, y} = erlang:append_element({x}, y),
    %% Append to empty tuple
    {hello} = erlang:append_element({}, hello),
    %% Append integer
    {1, 2, 3} = erlang:append_element({1, 2}, 3),
    %% Nested tuple
    {a, {b, c}} = erlang:append_element({a}, {b, c}),
    %% NERA-92: this formerly implicit Nera extension now requires erlang:.
    {a, b} = erlang:append_element({a}, b),
    ok.
