-module(receive_infinity).
-export([test/0]).
test() ->
    self() ! first,
    first = receive first -> first after 1000 -> error(first_timeout) end,
    Parent = self(),
    spawn(fun() -> timer:sleep(10), Parent ! second end),
    second = wait(infinity),
    spawn(fun() -> timer:sleep(10), Parent ! third end),
    third = receive third -> third after infinity -> error(infinite_timed_out) end,
    ok.
wait(T) -> receive M -> M after T -> error(infinite_timed_out) end.
