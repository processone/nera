%% EXPECT: binary pattern size overflow passed
%% NERA-79: constant size pre-binding must not panic or wrap before matching.
-module(binary_pattern_size_overflow).
-export([test/0]).

test() ->
    lists:foreach(fun({B, Expected}) ->
        Expected = case_size(B),
        Expected = head_size(B),
        Expected = fun_size(B),
        Expected = receive_size(B),
        Expected = try_size(B),
        Expected = catch_size(B)
    end, [{<<42>>, 42}, {<<>>, fail}]),
    <<42>> = binary_size(<<42>>),
    fail = binary_size(<<>>),
    <<42:7>> = bitstring_size(<<42:7>>),
    fail = bitstring_size(<<>>),
    fail = negative_size(<<42>>),
    lists:foreach(fun({Encoding, B}) ->
        162 = utf_minimum(Encoding, B),
        fail = utf_minimum(Encoding, <<>>)
    end, [{utf8, <<7,194,162>>}, {utf16, <<7,0,162>>},
          {utf32, <<7,0,0,0,162>>}]),
    io:format("binary pattern size overflow passed~n"),
    ok.

case_size(B) ->
    case B of
        <<_:1152921504606846975/unit:256>> -> impossible;
        <<_:1152921504606846976>> -> impossible;
        <<_:1152921504606846975>> -> impossible;
        <<_:0, X:1/unit:8>> -> X;
        _ -> fail
    end.

head_size(<<X:1152921504606846975/unit:256>>) -> X;
head_size(<<X:1/unit:8>>) -> X;
head_size(_) -> fail.

fun_size(B) ->
    F = fun(<<X:1152921504606846975/unit:256>>) -> X;
           (<<X:1/unit:8>>) -> X;
           (_) -> fail
        end,
    F(B).

receive_size(B) ->
    self() ! B,
    receive
        <<X:1152921504606846975/unit:256>> -> X;
        <<X:1/unit:8>> -> X;
        _ -> fail
    after 1000 -> error(missing_message)
    end.

try_size(B) ->
    try id(B) of
        <<X:1152921504606846975/unit:256>> -> X;
        <<X:1/unit:8>> -> X;
        _ -> fail
    catch _:_ -> error(unexpected_exception)
    end.

catch_size(B) ->
    try throw(B)
    catch
        throw:<<X:1152921504606846975/unit:256>> -> X;
        throw:<<X:1/unit:8>> -> X;
        throw:_ -> fail
    end.

binary_size(B) ->
    case B of
        <<X:1152921504606846975/binary-unit:256>> -> X;
        <<X:1/binary>> -> X;
        _ -> fail
    end.

bitstring_size(B) ->
    case B of
        <<X:1152921504606846976/bitstring>> -> X;
        <<X:7/bitstring>> -> X;
        _ -> fail
    end.

%% NERA-120 / review F1: a UTF minimum after a huge prefix must not wrap.
utf_minimum(utf8, B) ->
    case B of
        <<_:9223372036854775800, C/utf8>> -> C;
        <<_:1152921504606846975/unit:8, C/utf8>> -> C;
        <<_:1/unit:8, C/utf8>> -> C;
        _ -> fail
    end;
utf_minimum(utf16, B) ->
    case B of
        <<_:9223372036854775800, C/utf16>> -> C;
        <<_:1152921504606846975/unit:8, C/utf16>> -> C;
        <<_:1/unit:8, C/utf16>> -> C;
        _ -> fail
    end;
utf_minimum(utf32, B) ->
    case B of
        <<_:9223372036854775800, C/utf32>> -> C;
        <<_:1152921504606846975/unit:8, C/utf32>> -> C;
        <<_:1/unit:8, C/utf32>> -> C;
        _ -> fail
    end.

negative_size(B) -> case B of <<_:(-1)/unit:256>> -> impossible; _ -> fail end.
id(X) -> X.
