%% EXPECT: resume exceptions ok
-module(resume_exception).
-export([test/0]).

test() ->
    {'EXIT', timeout} = (catch delayed(exit)),
    thrown = (catch delayed(throw)),
    {'EXIT', {bad, Trace}} = (catch outer(error)),
    true = is_list(Trace),
    undefined = get(unreachable),
    handled = try outer(exit) catch exit:timeout -> handled end,
    handled = try delayed(error) catch error:bad ->
        receive after 1 -> handled end
    after put(cleanup, done) end,
    done = get(cleanup),
    Parent = self(),
    spawn(fun() -> receive after 5 -> Parent ! go end end),
    {'EXIT', awakened} = (catch message_exit()),
    ok = delayed(normal),
    io:format("resume exceptions ok~n").

outer(Kind) ->
    delayed(Kind),
    put(unreachable, true).

delayed(Kind) ->
    receive after 1 ->
        case Kind of
            exit -> exit(timeout);
            throw -> throw(thrown);
            error -> error(bad);
            normal -> ok
        end
    end.

message_exit() ->
    receive go -> exit(awakened) end.
