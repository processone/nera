-module(fun_pattern_complex).
-export([test/0]).

test() ->
    %% Capture variables from binary pattern
    Bin = id(<<1:8, 2:8, 3:8>>),
    case Bin of
        <<A:8, B:8, C:8>> ->
            F = fun() -> A + B + C end,
            6 = F()
    end,

    %% Capture variables from map pattern
    Map = id(#{x => 10, y => 20}),
    case Map of
        #{x := X, y := Y} ->
            G = fun(Z) -> X + Y + Z end,
            35 = G(5)
    end,

    %% Capture from tuple pattern with nested structure
    case id({ok, [1, 2, 3]}) of
        {ok, L} ->
            H = fun() -> L end,
            [1, 2, 3] = H()
    end,

    ok.

id(I) -> I.
