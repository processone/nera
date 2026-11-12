-module(probe).
-export([main/0]).
main() ->
    Values = lists:seq(1, 50), Misses = lists:seq(101, 110), Hits = lists:seq(11, 20),
    bench(immediate, Values, Hits),
    bench(disjoint_tags, [{n,N} || N <- Values], [{m,N} || N <- Hits]),
    bench(same_tag, [{n,N} || N <- Values], [{n,N} || N <- Misses]),
    bench(overlap, [{n,N} || N <- Values], [{n,N} || N <- Hits]),
    bench(duplicates, [{n,N rem 5} || N <- Values], [{n,N rem 5} || N <- Hits]),
    bench(nested, [{n,[N]} || N <- Values], [{n,[N]} || N <- Hits]),
    bench(small, [{n,1},{n,2}], [{n,2}]),
    bench(wide, [{n,0,0,0,0,0,0,0,0,N} || N <- Values],
        [{m,0,0,0,0,0,0,0,0,N} || N <- Hits]).
bench(Name, A, B) ->
    Expected = reference(A,B),
    loop(A,B,Expected,100),
    {Time,ok} = timer:tc(fun() -> loop(A,B,Expected,10000) end),
    io:format("~p ~p~n",[Name,Time]).
loop(_,_,_,0) -> ok;
loop(A,B,Expected,N) -> Expected = A -- B, loop(A,B,Expected,N-1).
reference(A,[]) -> A;
reference(A,[H|T]) -> reference(remove(A,H),T).
remove([],_) -> [];
remove([H|T],H) -> T;
remove([H|T],V) -> [H|remove(T,V)].
