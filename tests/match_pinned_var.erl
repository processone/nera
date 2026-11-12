%% Adapted from OTP compiler/test/match_SUITE.erl mutable_variables/1
%% (Apache 2.0). Once a variable is bound, matching it against a non-equal
%% value raises `{badmatch, _}`.
-module(match_pinned_var).
-export([test/0]).

test() ->
    %% Direct rebinding must raise badmatch
    ok = try
             X = id(1),
             X = id(2),
             should_not_reach
         catch
             error:{badmatch, 2} -> ok
         end,

    %% Tuple unpack where one element is pre-bound to a non-matching value
    ok = try
             Y = id(1),
             {Y, _} = id({2, 99}),
             should_not_reach
         catch
             error:{badmatch, {2, 99}} -> ok
         end,

    io:format("ok~n"),
    ok.

id(I) -> I.
