-module(binary_segment_domain).
-export([test/0]).
test() ->
    lists:foreach(fun(N) -> bad(fun() -> integer(N) end) end,
                  [1.5, 2.0, invalid, [], -1, -8, -1152921504606846976, -1152921504606846977]),
    bad(fun() -> compensated(-8) end),
    bad(fun() -> unit_zero(-1) end),
    bad(fun() -> byte_segment(1.5) end),
    limit(fun() -> wide(1152921504606846975) end),
    limit(fun() -> integer(1152921504606846976) end),
    limit(fun() -> total(1152921504606846975) end),
    limit(fun() -> integer(1152921504606846975) end),
    <<>> = integer(0),
    <<1:3>> = integer(3),
    <<1:16>> = integer(16),
    <<1:16>> = wide(1),
    <<1>> = byte_segment(1),
    caught = try <<1:1152921504606846975, 2:1>>, error(continued_after_limit)
             catch error:system_limit -> caught end,
    caught = try <<1:(size_value())>>, error(continued_after_invalid)
             catch error:badarg -> caught end,
    {Pid, Ref} = spawn_monitor(fun() -> integer(1.5), error(continued_after_invalid) end),
    receive {'DOWN', Ref, process, Pid, badarg} -> ok
    after 2000 -> error(missing_binary_error) end,
    ok.
integer(N) -> <<1:N>>.
compensated(N) -> <<1:N, 2:16>>.
unit_zero(N) -> <<1:N/unit:8, 2:8>>.
byte_segment(N) -> <<1:N/unit:8>>.
wide(N) -> <<1:N/unit:16>>.
total(N) -> <<1:N, 2:1>>.
size_value() -> 1.5.
bad(F) -> {'EXIT', {badarg, _}} = (catch F()), ok.
limit(F) -> {'EXIT', {system_limit, _}} = (catch F()), ok.
