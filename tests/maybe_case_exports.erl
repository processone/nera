%% EXPECT: maybe case exports ok
-module(maybe_case_exports).
-export([test/0]).

test() ->
    x = plain(id(1)),
    y = plain(id(2)),
    x = before_match(id(1), id({ok, x})),
    y = before_match(id(2), id({ok, y})),
    {failed, {ok, y}} = before_match(id(1), id({ok, y})),
    {failed, mismatch} = before_match(id(2), id(mismatch)),
    x = after_match(id({ok, 1})),
    y = after_match(id({ok, 2})),
    {failed, mismatch} = after_match(id(mismatch)),
    io:format("maybe case exports ok~n"),
    ok.

%% No conditional match: a later expression reads the case's shared binding.
plain(B) ->
    maybe
        case B of
            1 -> R = x;
            _ -> R = y
        end,
        R
    end.

%% The shared binding is also an input to a later conditional match.
before_match(B, Input) ->
    maybe
        case B of
            1 -> R = x;
            _ -> R = y
        end,
        {ok, R} ?= Input,
        R
    else
        Failed -> {failed, Failed}
    end.

%% A successful conditional match can precede the exporting case.
after_match(Input) ->
    maybe
        {ok, B} ?= Input,
        case B of
            1 -> R = x;
            _ -> R = y
        end,
        R
    else
        Failed -> {failed, Failed}
    end.

id(Value) -> Value.
