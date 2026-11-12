-module(process_flag_basic).
-export([test/0]).

%% Test: process_flag returns old value
test() ->
    false = process_flag(trap_exit, true),
    true = process_flag(trap_exit, false),
    false = process_flag(trap_exit, true),
    ok.
