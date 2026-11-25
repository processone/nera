-module(send_negative_small_int).
-export([test/0]).

%% EXPECT: send_negative_small_int_ok

test() ->
    Parent = self(),
    Min = id(-(1 bsl 60)),
    Parent ! {id(-3)},
    {-3} = next_message(),
    Parent ! {id(-1), Min, id(0), id(42)},
    {-1, Min, 0, 42} = next_message(),
    Parent ! id(-3),
    -3 = next_message(),
    spawn(fun() -> Parent ! id(-7) end),
    -7 = next_message(),
    spawn(fun() -> Parent ! {Min, id(-11)} end),
    {Min, -11} = next_message(),
    spawn(fun() -> Parent ! {tag, id(-13), {[Min], id(-15)}} end),
    {tag, -13, {[Min], -15}} = next_message(),
    spawn(fun() -> Parent ! {id(-17), id(-19), id(0), id(1), id(Min - 1)} end),
    {-17, -19, 0, 1, -1152921504606846977} = next_message(),
    io:format("send_negative_small_int_ok~n"),
    ok.

id(X) -> X.

next_message() ->
    receive Value -> Value
    after 5000 -> erlang:error(message_timeout)
    end.
