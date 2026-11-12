-module(char_escaped_ordinary).
-export([test/0]).

test() ->
    35 = $\#,
    [35,63,97,56,57,233] = [$\#,$\?,$\a,$\8,$\9,$\é],
    <<35,63,97>> = <<$\#,$\?,$\a>>,
    hash = classify(35),
    other = classify(36),
    %% Named and octal escapes keep their special meanings.
    [10,9,13,32,92,7,83,511] = [$\n,$\t,$\r,$\s,$\\,$\7,$\123,$\777],
    ok.

classify(C) when C =:= $\# -> hash;
classify(_) -> other.
