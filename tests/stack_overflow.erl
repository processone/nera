-module(stack_overflow).
-export([main/0]).

main() ->
    Res = (catch deep_recurse(0)),
    case Res of
        {'EXIT', {system_limit, _}} ->
            io:format("Stack overflow caught~n"),
            ok;
        _ ->
            io:format("Unexpected: ~p~n", [Res]),
            error
    end.

deep_recurse(N) ->
    1 + deep_recurse(N + 1).  %% Non-tail recursive
