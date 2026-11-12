%% EXPECT: ok
-module(universal_time).
-export([test/0]).

%% Bracket each call: crossing a second or midnight is valid. Run with a
%% non-UTC TZ as well to catch accidental local-time conversion.
test() ->
    ok = check(fun() -> erlang:universaltime() end),
    ok = check(fun() -> calendar:universal_time() end),
    ok = check(fun erlang:universaltime/0),
    ok = check(fun calendar:universal_time/0),
    ok = check(fun() -> apply(erlang, universaltime, []) end),
    ok = check(fun() -> apply(calendar, universal_time, []) end),
    io:format("ok~n").

check(F) ->
    Before = erlang:system_time(second),
    {{Y,Mo,D},{H,Mi,S}} = DateTime = F(),
    After = erlang:system_time(second),
    true = is_integer(Y),
    true = Mo >= 1 andalso Mo =< 12,
    true = D >= 1 andalso D =< 31,
    true = H >= 0 andalso H < 24,
    true = Mi >= 0 andalso Mi < 60,
    true = S >= 0 andalso S < 60,
    Unix = calendar:datetime_to_gregorian_seconds(DateTime)-62167219200,
    true = Before =< Unix andalso Unix =< After,
    ok.
