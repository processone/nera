%% EXPECT: integer_source_domain ok
-module(integer_source_domain).
-export([test/0]).
test() ->
    Max = (1 bsl 64) - 1,
    Min = -9223372036854775807 - 1,
    Max = unsigned(<<255,255,255,255,255,255,255,255>>),
    Min = signed(<<128,0,0,0,0,0,0,0>>),
    Max = unaligned(<<127,255,255,255,255,255,255,255,128>>),
    Ref = counters:new(1, []),
    ok = counters:put(Ref, 1, 1152921504606846975),
    ok = counters:add(Ref, 1, 1),
    1152921504606846976 = counters:get(Ref, 1),
    Nano = erlang:system_time(nanosecond),
    true = Nano > 1152921504606846975,
    Seconds = erlang:system_time(second),
    Delta = Seconds - Nano div 1000000000,
    true = Delta >= 0,
    true = Delta < 10,
    io:format("integer_source_domain ok~n"),
    ok.
unsigned(Bin) -> <<Value:64/unsigned>> = Bin, Value.
signed(Bin) -> <<Value:64/signed>> = Bin, Value.
unaligned(Bin) -> <<_:1, Value:64/unsigned, _:7>> = Bin, Value.
