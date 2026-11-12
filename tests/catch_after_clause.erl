%% Ported from OTP trycatch_SUITE concepts: `after` clause semantics.
-module(catch_after_clause).
-export([test/0]).

test() ->
    %% after runs on normal return
    ok = after_on_normal(),
    %% after runs on throw
    ok = after_on_throw(),
    %% after runs on error
    ok = after_on_error(),
    %% after runs on exit
    ok = after_on_exit(),
    %% after value is discarded; body value wins
    body_value = after_value_discarded(),
    %% try-of: after runs after the of clause too
    ok = after_runs_after_of_clause(),
    %% nested try/after: both afters run, outer first in stack order
    [inner, outer] = nested_after_order(),
    %% exception in after replaces the normal return
    {caught, after_boom} = after_exception_replaces_return(),
    %% exception in after replaces the caught exception
    {caught, after_boom} = after_exception_replaces_exception(),
    ok.

after_on_normal() ->
    Self = self(),
    try
        42
    after
        Self ! ran_normal
    end,
    receive ran_normal -> ok after 1000 -> error(no_msg) end.

after_on_throw() ->
    Self = self(),
    catch
        try
            throw(oops)
        after
            Self ! ran_throw
        end,
    receive ran_throw -> ok after 1000 -> error(no_msg) end.

after_on_error() ->
    Self = self(),
    catch
        try
            error(boom)
        after
            Self ! ran_error
        end,
    receive ran_error -> ok after 1000 -> error(no_msg) end.

after_on_exit() ->
    Self = self(),
    catch
        try
            exit(bye)
        after
            Self ! ran_exit
        end,
    receive ran_exit -> ok after 1000 -> error(no_msg) end.

after_value_discarded() ->
    try
        body_value
    after
        completely_ignored
    end.

after_runs_after_of_clause() ->
    Self = self(),
    _ = try id(3) of
            3 -> of_ran
        after
            Self ! after_after_of
        end,
    receive after_after_of -> ok after 1000 -> error(no_msg) end.

nested_after_order() ->
    Self = self(),
    try
        try
            ok
        after
            Self ! inner
        end
    after
        Self ! outer
    end,
    collect_msgs(2, []).

collect_msgs(0, Acc) -> lists:reverse(Acc);
collect_msgs(N, Acc) ->
    receive M -> collect_msgs(N-1, [M|Acc])
    after 1000 -> error(timeout)
    end.

after_exception_replaces_return() ->
    try
        try
            42
        after
            throw(after_boom)
        end
    catch
        throw:after_boom -> {caught, after_boom}
    end.

after_exception_replaces_exception() ->
    try
        try
            error(original)
        after
            throw(after_boom)
        end
    catch
        throw:after_boom -> {caught, after_boom};
        error:original -> {caught, original}
    end.

id(X) -> X.
