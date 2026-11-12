-module(xmod_string_utils).
-export([reverse/1, length_of/1]).

%% Helper module for multi-helper cross-module test.

reverse([]) -> [];
reverse([H|T]) -> reverse(T) ++ [H].

length_of([]) -> 0;
length_of([_|T]) -> 1 + length_of(T).
