%% EXPECT: ok
%% string:lowercase/1 as on OTP 29: full Unicode mapping per codepoint
%% without context rules. Expected values were taken from BEAM.
-module(string_lowercase).
-export([test/0]).
test() ->
    <<"abc">> = string:lowercase(<<"ABC">>),
    <<"abc ünï"/utf8>> = string:lowercase(<<"ABC Ünï"/utf8>>),
    "abc" = string:lowercase("ABC"),
    [97,98,99,32,252,110,239] = string:lowercase([65,66,67,32,220,110,239]),
    [] = string:lowercase([]),
    <<>> = string:lowercase(<<>>),
    "abcdef" = string:lowercase([<<"AB">>, "C", $D, [<<"EF">>]]),
    "abc" = string:lowercase([$A|<<"BC">>]),
    "ab" = string:lowercase([["A"],<<"B">>]),
    %% No final-sigma rule, and full mappings may grow the string.
    [945,963] = string:lowercase([913,931]),
    [945,963,32,945,963,945] = string:lowercase("ΑΣ ΑΣΑ"),
    <<105,204,135>> = string:lowercase(<<"İ"/utf8>>),
    %% Unchanged input comes back as given.
    B = <<"already lower"/utf8>>,
    true = B =:= string:lowercase(B),
    L = [$a, $b, 233],
    true = L =:= string:lowercase(L),
    {badarg, atom} = failure(fun() -> string:lowercase(atom) end),
    {badarg, 5} = failure(fun() -> string:lowercase(5) end),
    {badarg, <<255>>} = failure(fun() -> string:lowercase(<<255>>) end),
    {badarg, <<255>>} = failure(fun() -> string:lowercase(<<"aB",255>>) end),
    {badarg, <<255>>} = failure(fun() -> string:lowercase(["a", <<"B",255>>]) end),
    function_clause = failure(fun() -> string:lowercase([-1]) end),
    function_clause = failure(fun() -> string:lowercase([1114112]) end),
    function_clause = failure(fun() -> string:lowercase("A"++[bad]) end),
    function_clause = failure(fun() -> string:lowercase(["AB"|bad]) end),
    <<"x">> = apply(string, lowercase, [<<"X">>]),
    F = fun string:lowercase/1,
    "x" = F("X"),
    %% Every Unicode scalar value, mapped in one pass.
    All = [C || C <- lists:seq(0, 16#10FFFF), C < 16#D800 orelse C > 16#DFFF],
    Lower = string:lowercase(All),
    true = is_list(Lower),
    1112065 = length(Lower),
    ok = lowered_pins(Lower),
    io:format("ok~n").

%% Order-sensitive checksum of the full mapping, derived from OTP 29.
lowered_pins(Lower) ->
    case checksum(Lower, 1, 0) of
        Sum when Sum =:= 93842646 -> ok;
        Other -> {unexpected_checksum, Other}
    end.
checksum([], _, Acc) -> Acc;
checksum([C|T], I, Acc) -> checksum(T, I+1, (Acc + C*I) rem 1000000007).
failure(F) -> try F() of _ -> unexpected_return catch error:R -> R end.
