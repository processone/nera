%% EXPECT: ok
-module(nested_dynamic_tail_call).
-export([test/0, identity/1]).
test() ->
    F = fun() -> (fun() -> {wrapped, key(<<"name">>, fun escape/3)} end)() end,
    {wrapped, [[] | <<"name">>]} = F(),
    G = fun() -> (fun() -> {wrapped, remote(?MODULE, identity, 42)} end)() end,
    {wrapped, 42} = G(),
    H = fun() -> (fun() ->
        try key(ignored, fun(_, _, _) -> error(expected) end)
        catch error:expected -> caught end
    end)() end,
    caught = H(),
    ok = countdown(100000),
    io:format("ok~n").
key(String, Escape) -> Escape(String, String, 0).
escape(S, _, _) -> [[] | S].

remote(M, F, Arg) -> M:F(Arg).
identity(X) -> X.
countdown(0) -> ok;
countdown(N) -> F = fun countdown/1, F(N - 1).
