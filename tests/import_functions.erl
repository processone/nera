%% EXPECT: ok
-module(import_functions).
-import(lists, [reverse/1, reverse/2]).
-import(maps, [get/2]).
-export([test/0]).
test() ->
    [3, 2, 1] = reverse([1, 2, 3]),
    [3, 2, 1] = 'reverse'([1, 2, 3]),
    [2, 1, 3] = reverse([1, 2], [3]),
    value = get(key, #{key => value}),
    F = fun lists:reverse/1,
    [b, a] = F([a, b]),
    [y, x] = tail([x, y]),
    io:format("ok~n").
tail(X) -> reverse(X).
