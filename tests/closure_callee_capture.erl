%% EXPECT: ok
-module(closure_callee_capture).
-export([test/0]).
test() ->
    [F, G] = [fun() -> (fun() -> Expected end)() end || Expected <- [42, 99]],
    42 = F(),
    99 = G(),
    io:format("ok~n").
