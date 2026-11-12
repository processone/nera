%% Run on BEAM before and after `nera fmt --stdout`.
%% The unformatted program must print matched.
-module(formatter_guard_probe).
-export([start/0]).

start() -> io:format("~p~n", [classify(2, 1)]).

classify(A, B) when not (A < B) -> matched;
classify(_, _) -> fallback.
