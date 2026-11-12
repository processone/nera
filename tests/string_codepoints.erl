%% coding: utf-8
%% NERA-26: string expressions must be codepoint lists, not UTF-8 byte lists.
%% Expected lists checked with BEAM OTP 29; keep binary encoding separate.
%% EXPECT: string_codepoints_ok
-module(string_codepoints).
-export([test/0, readings/0]).

-define(GREETING, "héλ🤔").

test() ->
    lists:foreach(fun({Name, Expected, Actual}) ->
        case Actual =:= Expected of
            true -> ok;
            false -> erlang:error({wrong_codepoints, Name, Expected, Actual})
        end
    end, readings()),

    %% List operations see one integer per character, without normalization.
    4 = length(id(?GREETING)),
    955 = hd(id("λ")),
    [955,956] = id("λ") ++ id("μ"),
    [104,233,955,129300] = [C || C <- id(?GREETING)],
    #{"λ" := value} = id(#{[955] => value}),
    value = maps:get(id("λ"), #{[955] => value}),

    %% Existing list-string patterns must agree with expression construction.
    "λ" = id([955]),
    "\351" = id([233]),
    "\400\777" = id([256,511]),
    matched = classify(id("λ🤔")),
    matched = classify(id([955,129300])),
    other = classify(id([206,187,240,159,164,148])),
    [120] = prefix(id("λx")),
    [120] = prefix(id([955,120])),

    %% Original formatter trigger: literal Unicode padding and octal strings.
    [124,955,955,955,955,124] = lists:flatten(io_lib:format("|~4..λts|", ["λ"])),
    [233] = lists:flatten(io_lib:format("~s", ["\351"])),
    io:format("string_codepoints_ok~n"),
    ok.

%% Exported so the same source can produce numeric native/BEAM transcripts.
readings() ->
    [
        {empty, [], ""},
        {ascii, [104,101,108,108,111], "hello"},
        {latin1, [99,97,102,233], "café"},
        {greek, [955,956], "λμ"},
        {cjk, [26085,26412,35486], "日本語"},
        {supplementary, [65,129300,119070,90], "A🤔𝄞Z"},
        {combining, [101,769], "é"},
        {octal, [0,127,128,233,255,256,511], "\000\177\200\351\377\400\777"},
        {controls, [10,9,34,92], "\n\t\"\\"},
        {adjacent, [233,955,129300], "é" "λ" "🤔"},
        {macro, [104,233,955,129300], ?GREETING},
        {returned, [955,129300], literal()}
    ].

literal() -> "λ🤔".
classify("λ🤔") -> matched;
classify(_) -> other.
prefix("λ" ++ Rest) -> Rest.
id(X) -> X.
