%% EXPECT: garbage_collect_bif_ok
%% cow_hpack calls garbage_collect/0 before its oversized-integer assertion.
-module(garbage_collect_bif).
-export([test/0]).

test() ->
    Live = {<<"live-prefix">>, binary:copy(<<77>>, 1000), [a, #{b => [1,2,3]}]},
    put(gc_probe, Live),
    self() ! {kept, Live},
    true = garbage_collect(),
    true = erlang:garbage_collect(),
    true = apply(erlang, garbage_collect, []),
    F = fun erlang:garbage_collect/0,
    true = F(),
    Live = erase(gc_probe),
    receive {kept, Live} -> ok after 1000 -> error(lost_message) end,
    {<<"live-prefix">>, Bin, [a, #{b := [1,2,3]}]} = Live,
    1000 = byte_size(Bin),
    77 = binary:at(Bin, 999),
    io:format("garbage_collect_bif_ok~n"),
    ok.
