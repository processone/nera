-module(iolist_size).
-export([test/0]).

test() ->
    0 = iolist_size([]),
    0 = iolist_size(<<>>),
    2 = iolist_size([0,255]),
    6 = iolist_size([1,[2,<<3,4>>],[]|<<5,6>>]),
    3 = erlang:iolist_size(<<1,2,3>>),
    3 = apply(erlang, iolist_size, [[1,<<2,3>>]]),
    F = fun erlang:iolist_size/1,
    1 = F([7]),
    [0,1,2,3,64,65,256] = [iolist_size(binary:copy(<<7>>, N))
                             || N <- [0,1,2,3,64,65,256]],
    <<_:3, View:16/binary, _/bits>> = binary:copy(<<255>>, 100),
    16 = iolist_size(View),
    1 = iolist_size(deep(20000, [7])),
    ok = errors([1, -1, 256, undefined, {a}, 1.0, <<1:1>>,
                 [256], [-1], [1|2], [1|undefined], [<<1:1>>],
                 [[1|2]], [99999999999999999999]]),
    ok.

deep(0, Acc) -> Acc;
deep(N, Acc) -> deep(N - 1, [Acc]).

errors([]) -> ok;
errors([V|Rest]) ->
    badarg = try iolist_size(V) of _ -> unexpected_success catch error:R -> R end,
    errors(Rest).
