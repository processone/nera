%% EXPECT_COMPILE_ERROR: undefined record 'nope'
%% Regression: a pattern naming a record with no definition must be a
%% located compile error, not a pattern-compiler panic (exit 101).
-module(record_undefined_pattern).
-export([test/0]).

-record(req, {method, path}).

test() ->
    get = method(#req{method = get, path = <<"/">>}),
    ok.

method(#req{method = Method}) -> Method;
method(#nope{}) -> undefined.
