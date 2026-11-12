%% EXPECT: term_exact_equality_ok
-module(term_exact_equality).
-export([test/0]).
test() ->
    false = ([1] =:= [1.0]), true = ([1] =/= [1.0]), true = ([1] == [1.0]),
    false = ({[1]} =:= {[1.0]}), false = (#{key => [1]} =:= #{key => [1.0]}),
    false = (#{1 => a} =:= #{1.0 => a}),
    true = (#{a => [1], b => {2}} =:= maps:from_list([{b,{2}},{a,[1]}])),
    true = ([<<"a">>,{1}] =:= [<<"a">>,{1}]),
    io:format("term_exact_equality_ok~n").
