-module(atomic_array_bounds).
-export([test/0]).
test() ->
    A = atomics:new(3, []), C = counters:new(3, []),
    #{size := 3, memory := 24} = atomics:info(A),
    #{size := 3, memory := 24} = counters:info(C),
    ok = atomics:put(A, 1, 7), ok = atomics:put(A, 3, 9),
    ok = counters:put(C, 1, 11), ok = counters:put(C, 3, 13),
    lists:foreach(fun(I) ->
        bad(fun() -> atomics:get(A, I) end),
        bad(fun() -> atomics:put(A, I, 1) end),
        bad(fun() -> atomics:add(A, I, 1) end),
        bad(fun() -> atomics:sub(A, I, 1) end),
        bad(fun() -> atomics:exchange(A, I, 1) end),
        bad(fun() -> atomics:compare_exchange(A, I, 0, 1) end),
        bad(fun() -> counters:get(C, I) end),
        bad(fun() -> counters:put(C, I, 1) end),
        bad(fun() -> counters:add(C, I, 1) end),
        bad(fun() -> counters:sub(C, I, 1) end)
    end, [-1, 0, 4, invalid, []]),
    7 = atomics:get(A, 1), 0 = atomics:get(A, 2), 9 = atomics:get(A, 3),
    11 = counters:get(C, 1), 0 = counters:get(C, 2), 13 = counters:get(C, 3),
    ok.
bad(F) -> {'EXIT', {badarg, _}} = (catch F()), ok.
