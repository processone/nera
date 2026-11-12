%% Width/precision regression: EUnit timing and subsequent arguments must match BEAM.
%% EXPECT: format=[done in 0.019 s]|    a b|0000.019 tail|1.3 -1.3 -0.000|   1.23e+2 1.90e-2|  hel hel  |   **|00FF tail|aaa ~~~|   hi 0xff -16#FF [104,105] a b|fwrite=[done in 0.019 s]|    a b|0000.019 tail|1.3 -1.3 -0.000|   1.23e+2 1.90e-2|  hel hel  |   **|00FF tail|aaa ~~~|   hi 0xff -16#FF [104,105] a b|apply=    a b|fun=0.019 tail|fields complete
-module(io_format_fields).
-export([test/0]).

test() ->
    Fmt = "[done in ~.3f s]|~5w ~w|~*.*.*f ~w|~.1f ~.1f ~.3f|~10.3e ~.3g|~5.3s ~-5.3s|~5.2w|~*.*.*B ~w|~3c ~3~|~5.5s ~.16x ~.16# ~1000lp ~Kp ~w|",
    Args = [19/1000, a, b, 8, 3, $0, 0.019, tail,
            1.25, -1.25, -0.0, 123.0, 0.019, "hello", "hello", abcd,
            4, 16, $0, 255, tail, $a, "hi", 255, "0x", -255, "hi", ordered, a, b],
    Expected = "[done in 0.019 s]|    a b|0000.019 tail|1.3 -1.3 -0.000|   1.23e+2 1.90e-2|  hel hel  |   **|00FF tail|aaa ~~~|   hi 0xff -16#FF [104,105] a b|",
    Expected = lists:flatten(io_lib:format(Fmt, Args)),
    io:format("format="),
    ok = io:format(Fmt, Args),
    io:format("fwrite="),
    ok = io:fwrite(Fmt, Args),
    io:format("apply="),
    ok = apply(io, id(fwrite), ["~5w ~w|", [a,b]]),
    io:format("fun="),
    Write = fun io:format/2,
    ok = Write("~.*f ~w|", [3,0.019,tail]),
    fields(),
    %% Accepted non-UTF-8 Unicode strings must not stop either output API.
    ok = io:format("~0ts", [<<255,104>>]),
    ok = io:fwrite("~0ts", [<<255,104>>]),
    invalid(),
    io:format("fields complete~n").

fields() ->
    %% Oracle readings from OTP 29.0.4; exercise each field through io_lib's
    %% public interface, which must use the same renderer as both output APIs.
    Cases = [
        {"|~5w|~-5w|~0w|~.2w|~w|", [a,a,a,abcd,b], "|    a|a    ||**|b|"},
        {"|~*w|~w|", [-5,a,b], "|a    |b|"},
        {"|~-*w|~w|", [-5,a,b], "|    a|b|"},
        {"|~5..0w|~w|", [a,b], "|0000a|b|"},
        {"|~5s|~-5s|~.5s|~3s|~0s|", ["hi","hi","hi",hello,"abc"], "|   hi|hi   |hi   |hel||"},
        {"|~5.3s|~-5.3s|", ["a","a"], "|  a  |a    |"},
        {"|~5.5s|~-5.5s|~5.5ts|~-5.5ts|", ["hi","hi","hi","hi"], "|   hi|hi   |   hi|hi   |"},
        {"|~s|~s|", [["a",<<98>>,[$c]],<<233>>], [$|,$a,$b,$c,$|,233,$|]},
        {"|~4.1ts|~4..*ts|~w|", [[955,956],955,[955],tail], [ $|,32,32,32,955,$|,955,955,955,955,$|,$t,$a,$i,$l,$| ]},
        {[$|,$~,$4,$.,$.,955,$t,$s,$|], [[955]], [$|,955,955,955,955,$|]},
        {"|~2ts|", [[101,769,$x,$y]], [$|,101,769,$x,$|]},
        {"|~ts|~ts|", [<<255,104>>,<<206,187,255>>], [$|,255,104,$|,206,187,255,$|]},
        {"|~ts|", [[<<206,187>>,[<<255,104>>,<<195>>,<<169>>]]], [$|,955,255,104,195,169,$|]},
        {"|~4.4ts|", [<<255,104>>], [$|,32,32,255,104,$|]},
        {"|~8.3f|~-8.3f|~4.3f|~0f|", [0.019,0.019,0.019,0.019], "|   0.019|0.019   |****||"},
        {"|~f|~e|~g|", [1.0,1.0,1.0], "|1.000000|1.00000e+0|1.00000|"},
        {"|~.2f|~.2f|~.2e|~.1g|", [9.999,0.005,9.99,0.1], "|10.00|0.01|1.0e+1|0.1|"},
        {"|~.3g|~.3g|~.3g|~.3g|", [0.0,0.1,12.3,123.0], "|0.00e+0|0.100|12.3|1.23e+2|"},
        {"|~.20f|~.2e|", [1.0e-30,1.0e30], "|0.00000000000000000000|1.0e+30|"},
        {"|~2.16B|~2..0b|~w|", [255,123,tail], "|FF|**|tail|"},
        {"|~*.*.*B|~w|", [-4,16,$0,-7,tail], "|-700|tail|"},
        {"|~x|~.16X|~w|", [255,[["0x"]],-255,hex,tail], "|0x255|-hexFF|tail|"},
        {"|~#|~+|~.16#|~.16+|", [255,-255,255,-255], "|10#255|-10#255|16#FF|-16#ff|"},
        {"|~*.*.*x|~w|", [8,16,$0,-255,"0x",tail], "|000-0xff|tail|"},
        {"|~3.16x|~0x|~w|", [255,"0x",255,hex,tail], "|***||tail|"},
        {"|~lp|~tlp|~ltp|", ["hi",[955],["hi"]], "|[104,105]|[955]|[[104,105]]|"},
        {"|~kp|~Kp|~tKlkp|~w|", [#{a=>1},reversed,#{a=>1},ordered,a,tail], "|#{a => 1}|#{a => 1}|a|tail|"},
        {"|~5.2c|~.3c|~tc|~w|", [$a,$b,955,tail], [$|,32,32,32,$a,$a,$|,$b,$b,$b,$|,955,$|,$t,$a,$i,$l,$|]},
        {"~2n~w", [tail], "\n\ntail"},
        {"~*.*p|~w", [80,1,a,b], "a|b"}
    ],
    lists:foreach(fun({F,A,E}) -> E = lists:flatten(io_lib:format(F,A)) end, Cases).

invalid() ->
    lists:foreach(fun({Fmt,Args}) ->
        badarg = rejected(fun() -> io:format(Fmt,Args) end),
        badarg = rejected(fun() -> io:fwrite(Fmt,Args) end),
        badarg = rejected(fun() -> io_lib:format(Fmt,Args) end)
    end, [{"~*w",[bad,a]}, {"~.*f",[-1,1.0]}, {"~.0f",[1.0]},
          {"~.1e",[1.0]}, {"~f",[1]}, {"~.1B",[1]},
          {"~*.*.*f",[8,3]}, {"~5w",[]}, {"~5w",[a,b]},
          {"~5.6s",["abc"]}, {"~5w",[a|bad]}, {"~",[]},
          {"~.1x",[255,"0x"]}, {"~x",[255]}, {"~x",[255,<<"0x">>]},
          {"~x",[255,["0x"|bad]]}, {"~Kp",[]}, {"~Kp",[ordered]}]).

rejected(F) -> try F() of _ -> accepted catch error:badarg -> badarg end.
id(X) -> X.
