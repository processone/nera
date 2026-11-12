%% EXPECT: ok
-module(char_octal).
-export([test/0]).

test() ->
    [0,7,63,11,12,255,511] = [$\0,$\7,$\77,$\013,$\014,$\377,$\777],
    <<11,12,255>> = <<$\013,$\014,$\377>>,
    12 = $\013 + 1,
    true = is_space(11),
    true = is_space(12),
    false = is_space(13),
    io:format("ok~n").

is_space(C) when C =:= $\013; C =:= $\014 -> true;
is_space(_) -> false.
