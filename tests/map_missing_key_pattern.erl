-module(map_missing_key_pattern).
-export([test/0]).
test() ->
    missing = pick(#{}),
    missing = pick(#{other=>1}),
    {found,2} = pick(#{wanted=>2}),
    missing = case #{outer=>#{x=>1}} of #{outer:=#{wanted:=V}} -> {found,V}; _ -> missing end,
    missing = case #{a=>1} of #{a:=_,b:=_} -> found; _ -> missing end,
    {badmatch,#{other:=1}} = try #{wanted:=_} = #{other=>1} catch error:R -> R end,
    ok.
pick(#{wanted:=V}) -> {found,V};
pick(_) -> missing.
