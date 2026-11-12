%% EXPECT: ok
%% Depth-limited ~W and ~P, as on OTP 29. Nera printed the literal text "~P"
%% instead. OTP's EUnit reporter formats a cancelled test with ~tP and passes
%% that text to io:fwrite again, so the leftover "~P" made the reporter fail
%% with badarg while Cowlib's upstream suites ran natively. Expected values
%% were taken from BEAM. Map keys are integers so their order is fixed.
-module(io_format_depth).
-export([test/0]).
test() ->
    "[1,2|...]" = f("~W", [[1,2,3,4,5], 3]),
    "[1,2|...]" = f("~P", [[1,2,3,4,5], 3]),
    "[...]" = f("~W", [[1,2,3,4,5], 1]),
    "..." = f("~W", [[1,2,3,4,5], 0]),
    "[1,2,3,4,5]" = f("~W", [[1,2,3,4,5], -1]),
    "[1,2|...]" = f("~W", [[1,2|3], 3]),
    "[1,2|3]" = f("~W", [[1,2|3], 4]),
    "{1,2,...}" = f("~W", [{1,2,3,4}, 3]),
    "{...}" = f("~P", [{1,2,3,4}, 1]),
    "{a,[b|...]}" = f("~W", [{a,[b,{c,[d]}]}, 4]),
    "{a,[...]}" = f("~P", [{a,[b,{c,[d]}]}, 3]),
    "<<1,2,...>>" = f("~W", [<<1,2,3,4,5>>, 3]),
    "<<...>>" = f("~P", [<<1,2,3,4,5>>, 1]),
    "<<1,2,...>>" = f("~W", [<<1,2,3:4>>, 3]),
    "<<1,2,3:4>>" = f("~W", [<<1,2,3:4>>, 4]),
    "<<7>>" = f("~W", [<<7>>, 2]),
    %% Large binaries are stored off-heap; a match on one gives a sub-binary.
    Large = binary:copy(<<5>>, 100),
    "<<5,5,...>>" = f("~W", [Large, 3]),
    <<_:8, Rest/binary>> = Large,
    "<<5,...>>" = f("~P", [Rest, 2]),
    %% ~W and ~P differ only for maps.
    "#{}" = f("~W", [#{1 => a, 2 => [1,2,3]}, 1]),
    "#{...}" = f("~P", [#{1 => a, 2 => [1,2,3]}, 1]),
    "#{1 => a,2 => [1|...],...}" = f("~W", [#{1 => a, 2 => [1,2,3]}, 3]),
    "#{1 => a,2 => [1|...]}" = f("~P", [#{1 => a, 2 => [1,2,3]}, 3]),
    "#{1 => a,...}" = f("~W", [#{1 => a}, 2]),
    "#{1 => a}" = f("~P", [#{1 => a}, 2]),
    "#{1 => a,2 => b,3 => c,...}" = f("~W", [#{1 => a, 2 => b, 3 => c}, 4]),
    "#{1 => a,2 => b,3 => c}" = f("~P", [#{1 => a, 2 => b, 3 => c}, 4]),
    "[]" = f("~W", [[], 1]),
    "{}" = f("~W", [{}, 1]),
    "#{}" = f("~W", [#{}, 1]),
    "<<>>" = f("~W", [<<>>, 1]),
    "abc" = f("~W", [abc, 1]),
    "     [1|...]|" = f("~12W|", [[1,2,3], 2]),
    "[1|...]     |" = f("~-12W|", [[1,2,3], 2]),
    "****|" = f("~4W|", [[1,2,3], 2]),
    "<[[[...]]]>" = f("<~tP>", [[[[[x]]]], 3]),
    %% EUnit's second pass: formatted text used as a format string.
    Reason = io_lib:format("**error:~tP", [{badmatch, [1,2,3]}, 3]),
    "**error:{badmatch,[...]}" = f(Reason, []),
    badarg = failure(fun() -> f("~W", [x]) end),
    badarg = failure(fun() -> f("~P", [x, y]) end),
    io:format("ok~n").
f(Format, Args) -> lists:flatten(io_lib:format(Format, Args)).
failure(F) -> try F() of _ -> unexpected_return catch error:R -> R end.
