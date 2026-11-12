%% EXPECT: application_start_ignore_ok
%% MODULES: _ignored_app.erl
%% Regression: when an application's start/2 returns a value that is not a
%% start result, such as the ignore its top supervisor returned,
%% application:start/1 returns {error, {bad_return, {{Mod, start, [Type,
%% Args]}, Value}}}, as on BEAM (OTP 29). Before the fix, native Nera crashed
%% the caller with case_clause once supervisor:start_link/2 returned ignore.
-module(application_start_ignore).
-export([test/0]).

test() ->
    {error, {bad_return, {{ignored_app, start, [normal, []]}, ignore}}} =
        application:start(ignored),
    io:format("application_start_ignore_ok~n"),
    ok.
