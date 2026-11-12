-module(exception_before_guard).
-export([test/0]).
test() ->
 R=try outer(#{host=>h}) catch C:E->{C,E} end,
 {throw,{error,invalid_map,#{host:=h}}}=R,ok.
outer(M) -> validate(M), case maps:find(host,M) of {ok,H} when is_integer(H) -> a; error -> b end.
validate(M) ->
 case maps:is_key(path,M) andalso lists:all(fun(K)->lists:member(K,[scheme,host,path]) end,maps:keys(M)) of
 true->ok; false->fail(invalid_map,M) end.
fail(R,M) -> throw({error,R,M}).
