%% EXPECT: import_resolution_global_ok
%% NERA-92: global suppression and legal explicit erlang reimports in guards.
-module(import_resolution_global).
-export([test/0]).
-compile([debug_info, no_auto_import]).
-import(erlang, [length/1, self/0, is_pid/1]).

test() ->
    2 = count([a,b]),
    local_throw = throw(ignored),
    local_error = error(ignored),
    local_size = byte_size(<<1>>),
    true = is_pid(self()),
    badarg = try ?MODULE:module_info(unknown) catch error:badarg -> badarg end,
    io:format("import_resolution_global_ok~n"),
    ok.
count(X) when length(X) > 0 -> length(X).
throw(_) -> local_throw.
error(_) -> local_error.
byte_size(_) -> local_size.
