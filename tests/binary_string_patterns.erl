%% coding: utf-8
%% NERA-71: binary-pattern strings expand to codepoints, not UTF-8 bytes.
%% Run this same source on native Nera and BEAM OTP 29. Default 8-bit
%% patterns above 255 never match, even though construction truncates them.
%% EXPECT: binary_string_patterns_ok
-module(binary_string_patterns).
-export([test/0, readings/0]).

test() ->
    lists:foreach(fun({Name, Expected, Actual}) ->
        case Actual =:= Expected of
            true -> ok;
            false -> erlang:error({wrong_string_pattern, Name, Expected, Actual})
        end
    end, readings()),
    %% Exact patterns also agree with the equivalent expressions.
    <<"é">> = id(<<"é">>),
    <<"λ"/utf8>> = id(<<"λ"/utf8>>),
    no = exact_latin(id(<<233,0>>)),
    no = exact_utf8(id(<<206,187,0>>)),
    %% Shifted input exercises both the matcher and body pre-binding offsets.
    {7, <<120>>} = shifted(id(<<5:3,233,206,187,240,159,164,148,7,120>>)),
    %% Other shared callers must use the same expansion and tail offsets.
    F = fun(<<"λ"/utf8, R/binary>>) -> R; (_) -> no end,
    <<120>> = F(id(<<206,187,120>>)),
    [<<120>>] = [R || <<"é", R/binary>> <- id([<<195,169,120>>, <<233,120>>])],
    io:format("binary_string_patterns_ok~n"),
    ok.

%% Each reading runs through case, function head and match-expression lowering.
%% The exported readings give a numeric native/BEAM transcript of the results.
readings() ->
    [{Name, lists:duplicate(3, Expected),
      [result(Fun, Kind, Bin) || Fun <- [fun case_pattern/2,
                                       fun head_pattern/2,
                                       fun match_pattern/2]]}
     || {Name, Kind, Bin, Expected} <- cases()].

cases() ->
    [
        {ascii, ascii, <<"A\000!tail">>, {matched, <<"tail">>}},
        {empty, empty, <<120>>, {matched, <<120>>}},
        {empty_input, empty, <<>>, {matched, <<>>}},
        {latin_expression, latin, <<"é",120>>, {matched, <<120>>}},
        {latin_numeric, latin, <<233,120>>, {matched, <<120>>}},
        {latin_empty_tail, latin, <<233>>, {matched, <<>>}},
        {latin_utf8_bytes, latin, <<195,169,120>>, no},
        {latin_short, latin, <<>>, no},
        {latin_nonbinary, latin, invalid, no},
        {octal_byte_expression, octal_byte, <<"\377",120>>, {matched, <<120>>}},
        {octal_byte_utf8_bytes, octal_byte, <<195,191,120>>, no},
        {octal_400_expression, octal_high, <<"\400",120>>, no},
        {octal_400_utf8_bytes, octal_high, <<196,128,120>>, no},
        {octal_777_expression, octal_max, <<"\777",120>>, no},
        {octal_777_utf8_bytes, octal_max, <<199,191,120>>, no},
        {greek_default_expression, greek_default, <<"λ",120>>, no},
        {greek_default_utf8_bytes, greek_default, <<206,187,120>>, no},
        {utf8_expression, utf8, <<"Aéλ€🤔"/utf8,120>>, {matched, <<120>>}},
        {utf8_numeric, utf8, <<65,195,169,206,187,226,130,172,240,159,164,148,120>>,
         {matched, <<120>>}},
        {utf8_empty_tail, utf8, <<"Aéλ€🤔"/utf8>>, {matched, <<>>}},
        {utf8_wrong_character, utf8, <<"Aéμ€🤔"/utf8,120>>, no},
        {utf8_truncated, utf8, <<65,195,169,206,187,226,130,172,240,159,164>>, no},
        {utf8_malformed, utf8, <<65,195,169,206,187,226,130,172,255,255,255,255>>, no},
        {utf8_octal, utf8_octal, <<"\400\777"/utf8,120>>, {matched, <<120>>}},
        %% UTF-16/32 patterns use the same expansion; integer segments are the
        %% reference construction for their codepoints and endianness.
        {utf16_big, utf16_big, <<955/utf16,129300/utf16,120>>, {matched, <<120>>}},
        {utf16_little, utf16_little, <<955/utf16-little,129300/utf16-little,120>>,
         {matched, <<120>>}},
        {utf32_big, utf32_big, <<955/utf32,129300/utf32,120>>, {matched, <<120>>}},
        {utf32_little, utf32_little, <<955/utf32-little,129300/utf32-little,120>>,
         {matched, <<120>>}}
    ].

result(Fun, Kind, Bin) ->
    try Fun(Kind, Bin) catch error:{badmatch, Bin} -> no end.

case_pattern(ascii, B) -> case B of <<"A\000!", R/binary>> -> {matched,R}; _ -> no end;
case_pattern(empty, B) -> case B of <<"", R/binary>> -> {matched,R}; _ -> no end;
case_pattern(latin, B) -> case B of <<"é", R/binary>> -> {matched,R}; _ -> no end;
case_pattern(octal_byte, B) -> case B of <<"\377", R/binary>> -> {matched,R}; _ -> no end;
case_pattern(octal_high, B) -> case B of <<"\400", R/binary>> -> {matched,R}; _ -> no end;
case_pattern(octal_max, B) -> case B of <<"\777", R/binary>> -> {matched,R}; _ -> no end;
case_pattern(greek_default, B) -> case B of <<"λ", R/binary>> -> {matched,R}; _ -> no end;
case_pattern(utf8, B) -> case B of <<"Aéλ€🤔"/utf8, R/binary>> -> {matched,R}; _ -> no end;
case_pattern(utf8_octal, B) -> case B of <<"\400\777"/utf8, R/binary>> -> {matched,R}; _ -> no end;
case_pattern(utf16_big, B) -> case B of <<"λ🤔"/utf16, R/binary>> -> {matched,R}; _ -> no end;
case_pattern(utf16_little, B) -> case B of <<"λ🤔"/utf16-little, R/binary>> -> {matched,R}; _ -> no end;
case_pattern(utf32_big, B) -> case B of <<"λ🤔"/utf32, R/binary>> -> {matched,R}; _ -> no end;
case_pattern(utf32_little, B) -> case B of <<"λ🤔"/utf32-little, R/binary>> -> {matched,R}; _ -> no end.

head_pattern(ascii, <<"A\000!", R/binary>>) -> {matched,R};
head_pattern(empty, <<"", R/binary>>) -> {matched,R};
head_pattern(latin, <<"é", R/binary>>) -> {matched,R};
head_pattern(octal_byte, <<"\377", R/binary>>) -> {matched,R};
head_pattern(octal_high, <<"\400", R/binary>>) -> {matched,R};
head_pattern(octal_max, <<"\777", R/binary>>) -> {matched,R};
head_pattern(greek_default, <<"λ", R/binary>>) -> {matched,R};
head_pattern(utf8, <<"Aéλ€🤔"/utf8, R/binary>>) -> {matched,R};
head_pattern(utf8_octal, <<"\400\777"/utf8, R/binary>>) -> {matched,R};
head_pattern(utf16_big, <<"λ🤔"/utf16, R/binary>>) -> {matched,R};
head_pattern(utf16_little, <<"λ🤔"/utf16-little, R/binary>>) -> {matched,R};
head_pattern(utf32_big, <<"λ🤔"/utf32, R/binary>>) -> {matched,R};
head_pattern(utf32_little, <<"λ🤔"/utf32-little, R/binary>>) -> {matched,R};
head_pattern(_, _) -> no.

match_pattern(ascii, B) -> <<"A\000!", R/binary>> = B, {matched,R};
match_pattern(empty, B) -> <<"", R/binary>> = B, {matched,R};
match_pattern(latin, B) -> <<"é", R/binary>> = B, {matched,R};
match_pattern(octal_byte, B) -> <<"\377", R/binary>> = B, {matched,R};
match_pattern(octal_high, B) -> <<"\400", R/binary>> = B, {matched,R};
match_pattern(octal_max, B) -> <<"\777", R/binary>> = B, {matched,R};
match_pattern(greek_default, B) -> <<"λ", R/binary>> = B, {matched,R};
match_pattern(utf8, B) -> <<"Aéλ€🤔"/utf8, R/binary>> = B, {matched,R};
match_pattern(utf8_octal, B) -> <<"\400\777"/utf8, R/binary>> = B, {matched,R};
match_pattern(utf16_big, B) -> <<"λ🤔"/utf16, R/binary>> = B, {matched,R};
match_pattern(utf16_little, B) -> <<"λ🤔"/utf16-little, R/binary>> = B, {matched,R};
match_pattern(utf32_big, B) -> <<"λ🤔"/utf32, R/binary>> = B, {matched,R};
match_pattern(utf32_little, B) -> <<"λ🤔"/utf32-little, R/binary>> = B, {matched,R}.

exact_latin(<<"é">>) -> matched;
exact_latin(_) -> no.
exact_utf8(<<"λ"/utf8>>) -> matched;
exact_utf8(_) -> no.
shifted(<<5:3,"é","λ🤔"/utf8,N:8,R/binary>>) -> {N,R}.
id(X) -> X.
