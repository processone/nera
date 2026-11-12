%% EXPECT: lc_generator_pattern_ok
-module(lc_generator_pattern).
-export([test/0]).

test() ->
    [] = select([{<<"other">>, 7}]),
    [1, 2] = select([{<<"pick">>, 1}, {<<"other">>, 9}, {<<"pick">>, 2}]),
    [3] = select([ignored, {}, {<<"pick">>, 3}, {<<"pick">>, 4, extra}]),
    [1] = [V || {wanted, V} <- [{other, bad}, {wanted, 1}], V > 0],
    [4] = [V || {V, V} <- [{2, 3}, {4, 4}]],
    [{1, 2}] = [{X, Y} || {wanted, X} <- [{other, 0}, {wanted, 1}],
                         {wanted, Y} <- [{other, 0}, {wanted, 2}]],
    [5] = [V || #{value := V} <- [#{other => 0}, ignored, #{value => 5}]],
    [<<2>>] = [R || <<1, R/binary>> <- [<<1, 2>>, <<3, 4>>, ignored]],
    [{<<2>>, 9}] = [{R, Y} || <<1, R/binary>> <- [<<1, 2>>],
                             {wanted, Y} <- [{wanted, 9}]],
    <<1, 2>> = <<<<V>> || {wanted, V} <- [{other, 8}, {wanted, 1}, {wanted, 2}]>>,
    %% Generator variables intentionally shadow bindings outside their scope.
    Shadow = outside,
    [inside] = [Shadow || {wanted, Shadow} <- [{wanted, inside}, {other, bad}]],
    outside = Shadow,
    io:format("lc_generator_pattern_ok~n"),
    ok.

select(Items) -> [Value || {<<"pick">>, Value} <- Items].
