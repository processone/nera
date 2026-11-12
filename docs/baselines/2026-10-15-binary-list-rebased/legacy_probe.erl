-module(probe).
-export([main/0]).
main() ->
    lists:foreach(fun(Mode) ->
        binary_loop(Mode, 16, 100),
        {Time, 200000} = timer:tc(fun() -> binary_loop(Mode, 16, 100000) end),
        io:format("~p ~p~n", [Mode, Time])
    end, [constant, dynamic, unit, multi]),
    Left = lists:seq(1, 200), Right = lists:seq(101, 120),
    bench(append, Left, Right, 2200000),
    bench(subtract, Left, Right, 1800000),
    bench(structural, [{n, N} || N <- lists:seq(1, 50)], [{m,N} || N <- lists:seq(1, 10)], 500000).
binary_loop(Mode, Size, N) -> binary_loop(Mode, Size, N, 0).
binary_loop(_, _, 0, Acc) -> Acc;
binary_loop(Mode, Size, N, Acc) ->
    B = case Mode of
        constant -> <<1:16>>;
        dynamic -> <<1:Size>>;
        unit -> <<1:(Size div 8)/unit:8>>;
        multi -> <<1:(Size div 2), 2:(Size div 2)>>
    end,
    binary_loop(Mode, Size, N - 1, Acc + byte_size(B)).
bench(Mode, A, B, Expected) ->
    list_loop(Mode, A, B, 10),
    {Time, Expected} = timer:tc(fun() -> list_loop(Mode, A, B, 10000) end),
    io:format("~p ~p~n", [Mode, Time]).
list_loop(Mode, A, B, N) -> list_loop(Mode, A, B, N, 0).
list_loop(_, _, _, 0, Acc) -> Acc;
list_loop(Mode, A, B, N, Acc) ->
    L = case Mode of append -> A ++ B; _ -> A -- B end,
    list_loop(Mode, A, B, N - 1, Acc + length(L)).
