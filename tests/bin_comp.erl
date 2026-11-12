-module(bin_comp).
-export([test/0]).

test() ->
    %% Basic binary comprehension from list
    <<1,2,3>> = << <<X>> || X <- [1,2,3] >>,
    %% With filter
    <<2,4>> = << <<X>> || X <- [1,2,3,4], X rem 2 =:= 0 >>,
    %% Multi-byte segments
    <<0,1,0,2,0,3>> = << <<X:16>> || X <- [1,2,3] >>,
    %% Binary generator
    <<2,4,6>> = << <<(X*2)>> || <<X>> <= <<1,2,3>> >>,
    %% Nested expressions
    <<10,20,30>> = << <<(X*10)>> || X <- [1,2,3] >>,
    %% Empty input
    <<>> = << <<X>> || X <- [] >>,
    ok.
