%% coding: utf-8
%% NERA-104: a string in binary construction is one segment per codepoint,
%% each with the string's own size, unit, type and flags, so <<"ab":16>> is
%% <<$a:16, $b:16>>. Run this same source on native Nera and BEAM OTP 29.
%% Literal size 8 with another unit or a binary type is left out: there the
%% OTP 29 compiler takes a byte shortcut that its own evaluator does not.
%% EXPECT: binary_string_construction_ok
-module(binary_string_construction).
-export([test/0, readings/0]).

test() ->
    lists:foreach(fun({Name, Expected, Actual}) ->
        case Actual =:= Expected of
            true -> ok;
            false -> erlang:error({wrong_string_construction, Name, Expected, Actual})
        end
    end, readings()),
    io:format("binary_string_construction_ok~n"),
    ok.

%% Each reading is {Name, Expected, Actual}; the exported list gives a numeric
%% native/BEAM transcript. Constant binaries are folded by erlc, so the
%% variable cases also cover its runtime construction.
readings() ->
    [{Name, Expected, result(Fun)} || {Name, Expected, Fun} <- cases()].

cases() ->
    X = id(120), N0 = id(0), N4 = id(4), N8 = id(8), N16 = id(16),
    N32 = id(32), Neg = id(-1), Atom = id(sixteen),
    [
        %% UTF-16 and UTF-32: endianness and surrogate pairs. Native is the
        %% machine's byte order, when building and when matching.
        {utf16_greek, {ok, <<3,187>>}, fun() -> <<"λ"/utf16>> end},
        {utf32_greek, {ok, <<0,0,3,187>>}, fun() -> <<"λ"/utf32>> end},
        {utf16_big, {ok, <<0,97,3,187,216,62,221,20>>}, fun() -> <<"aλ🤔"/utf16>> end},
        {utf16_little, {ok, <<97,0,187,3,62,216,20,221>>}, fun() -> <<"aλ🤔"/utf16-little>> end},
        {utf16_native, {ok, native(<<0,97,3,187>>, <<97,0,187,3>>)}, fun() -> <<"aλ"/utf16-native>> end},
        {utf32_big, {ok, <<0,0,0,97,0,0,3,187,0,1,249,20>>}, fun() -> <<"aλ🤔"/utf32>> end},
        {utf32_little, {ok, <<97,0,0,0,187,3,0,0,20,249,1,0>>}, fun() -> <<"aλ🤔"/utf32-little>> end},
        {utf32_native, {ok, native(<<0,0,0,97,0,0,3,187>>, <<97,0,0,0,187,3,0,0>>)},
         fun() -> <<"aλ"/utf32-native>> end},
        {utf16_native_match, {ok, {955, 129300}}, fun() ->
            <<A/utf16-native, B/utf16-native>> =
                id(native(<<3,187,216,62,221,20>>, <<187,3,62,216,20,221>>)),
            {A, B} end},
        {utf32_native_match, {ok, {955, 129300}}, fun() ->
            <<A/utf32-native, B/utf32-native>> =
                id(native(<<0,0,3,187,0,1,249,20>>, <<187,3,0,0,20,249,1,0>>)),
            {A, B} end},
        {utf16_octal, {ok, <<1,0,1,255>>}, fun() -> <<"\400\777"/utf16>> end},
        {utf16_variable, {ok, <<3,187,216,62,221,20,120>>}, fun() -> <<"λ🤔"/utf16, X>> end},
        {utf16_little_variable, {ok, <<187,3,62,216,20,221,120>>}, fun() -> <<"λ🤔"/utf16-little, X>> end},
        {utf32_variable, {ok, <<0,0,3,187,120>>}, fun() -> <<"λ"/utf32, X>> end},
        {utf32_little_variable, {ok, <<187,3,0,0,120>>}, fun() -> <<"λ"/utf32-little, X>> end},
        {utf_empty, {ok, <<120>>}, fun() -> <<""/utf16, ""/utf32, X>> end},
        %% Literal sizes, units and flags apply to every codepoint.
        {sized16_latin, {ok, <<0,233>>}, fun() -> <<"é":16>> end},
        {sized16_greek, {ok, <<0,97,3,187>>}, fun() -> <<"aλ":16>> end},
        {sized16_little, {ok, <<97,0,187,3>>}, fun() -> <<"aλ":16/little>> end},
        {sized16_native, {ok, native(<<0,97,3,187>>, <<97,0,187,3>>)}, fun() -> <<"aλ":16/native>> end},
        {sized16_truncates, {ok, <<249,20>>}, fun() -> <<"🤔":16>> end},
        {sized24_signed, {ok, <<0,0,97,0,3,187>>}, fun() -> <<"aλ":24/signed>> end},
        {sized4, {ok, <<18>>}, fun() -> <<"ab":4>> end},
        {sized3, {ok, <<10:6>>}, fun() -> <<"ab":3>> end},
        {sized_unit, {ok, <<0,97,0,98>>}, fun() -> <<"ab":2/unit:8>> end},
        {sized0, {ok, <<>>}, fun() -> <<"ab":0>> end},
        {sized16_octal, {ok, <<1,0,1,255>>}, fun() -> <<"\400\777":16>> end},
        {sized16_variable, {ok, <<0,233,120>>}, fun() -> <<"é":16, X>> end},
        {sized16_little_variable, {ok, <<97,0,187,3,120>>}, fun() -> <<"aλ":16/little, X>> end},
        {sized_unit_variable, {ok, <<0,97,0,98,120>>}, fun() -> <<"ab":2/unit:8, X>> end},
        {sized_empty, {ok, <<120>>}, fun() -> <<"":16, X>> end},
        %% Float types write each codepoint as a float.
        {float64, {ok, <<64,88,64,0,0,0,0,0,64,88,128,0,0,0,0,0>>}, fun() -> <<"ab"/float>> end},
        {float32, {ok, <<66,194,0,0>>}, fun() -> <<"a":32/float>> end},
        {float_unit, {ok, <<66,194,0,0>>}, fun() -> <<"a":1/float-unit:32>> end},
        {float_variable, {ok, <<66,194,0,0,120>>}, fun() -> <<"a":32/float, X>> end},
        %% Dynamic sizes are evaluated once and checked, even for "".
        {dynamic16, {ok, <<0,97,3,187>>}, fun() -> <<"aλ":N16>> end},
        {dynamic4, {ok, <<18>>}, fun() -> <<"ab":N4>> end},
        {dynamic_zero, {ok, <<>>}, fun() -> <<"ab":N0>> end},
        {dynamic_unit, {ok, <<0,97,0,98>>}, fun() -> <<"ab":N8/unit:2>> end},
        {dynamic_little, {ok, <<97,0,187,3,120>>}, fun() -> <<"aλ":N16/little, X>> end},
        {dynamic_float, {ok, <<66,194,0,0,66,196,0,0>>}, fun() -> <<"ab":N32/float>> end},
        {dynamic_float_width, {error, badarg}, fun() -> <<"ab":N8/float>> end},
        {dynamic_negative, {error, badarg}, fun() -> <<"ab":Neg>> end},
        {dynamic_atom, {error, badarg}, fun() -> <<"ab":Atom>> end},
        {dynamic_once, {ok, {<<0,97,0,98,0,99>>, 1}}, fun size_once/0},
        {empty_dynamic16, {ok, <<>>}, fun() -> <<"":N16>> end},
        {empty_negative, {error, badarg}, fun() -> <<"":Neg>> end},
        {empty_atom, {error, badarg}, fun() -> <<"":Atom, X>> end},
        {empty_negative_try, {ok, {caught, badarg}}, fun() -> inline_try(Neg) end},
        %% Review R1: a size or value that fails inside try keeps its own
        %% reason; the segment check must not replace badarith with badarg.
        {try_string_size_badarith, {ok, {caught, badarith}}, fun() -> arith_try(string, N0) end},
        {try_string_float_size_badarith, {ok, {caught, badarith}}, fun() -> arith_try(string_float, N0) end},
        {try_string_binary_size_badarith, {ok, {caught, badarith}}, fun() -> arith_try(string_binary, N0) end},
        {try_empty_size_badarith, {ok, {caught, badarith}}, fun() -> arith_try(empty, N0) end},
        {try_empty_binary_size_badarith, {ok, {caught, badarith}}, fun() -> arith_try(empty_binary, N0) end},
        {try_integer_size_badarith, {ok, {caught, badarith}}, fun() -> arith_try(integer, N0) end},
        {try_float_size_badarith, {ok, {caught, badarith}}, fun() -> arith_try(float, N0) end},
        {try_binary_size_badarith, {ok, {caught, badarith}}, fun() -> arith_try(binary, N0) end},
        {try_binary_value_badarith, {ok, {caught, badarith}}, fun() -> arith_try(binary_value, N0) end},
        {try_size_ok, {ok, {built, <<0,97,0,98>>}}, fun() -> arith_try(string, id(1)) end},
        %% A codepoint is never a binary: these fail as on BEAM.
        {binary, {error, badarg}, fun() -> <<"ab"/binary>> end},
        {bits, {error, badarg}, fun() -> <<"ab"/bits>> end},
        {binary_sized, {error, badarg}, fun() -> <<"ab":N16/binary>> end},
        {binary_empty, {error, badarg}, fun() -> <<""/binary>> end},
        {binary_empty_sized, {error, badarg}, fun() -> <<"":16/binary, X>> end},
        %% Defaults keep 8-bit truncation; /utf8 keeps UTF-8.
        {default_greek, {ok, <<187>>}, fun() -> <<"λ">> end},
        {default_variable, {ok, <<97,233,120>>}, fun() -> <<"aé", X>> end},
        {utf8, {ok, <<97,206,187,240,159,164,148>>}, fun() -> <<"aλ🤔"/utf8>> end},
        {utf8_variable, {ok, <<206,187,120>>}, fun() -> <<"λ"/utf8, X>> end},
        %% Other segments, shifted offsets, guards and comprehensions.
        {mixed, {ok, <<1,3,187,233,0,2>>}, fun() -> <<1, "λ"/utf16, "é":16/little, 2>> end},
        {shifted, {ok, <<160,119,105>>}, fun() -> <<5:3, "λ"/utf16, "é":5>> end},
        {shifted_variable, {ok, <<160,119,105,0:3>>}, fun() -> <<5:3, "λ"/utf16, "é":5, X:3>> end},
        {guard_utf16, {ok, yes}, fun() -> guard_utf16(id(<<3,187>>)) end},
        {guard_size, {ok, yes}, fun() -> guard_size(id(<<0,233>>), N16) end},
        {guard_bad_size, {ok, no}, fun() -> guard_size(id(<<0,233>>), Neg) end},
        {comprehension, {ok, <<233,0,233,0>>}, fun() -> << <<"é":16/little>> || _ <- id([1,2]) >> end}
    ].

result(Fun) ->
    try {ok, Fun()} catch error:Reason -> {error, Reason} end.

%% Same-function try: the failed size check reaches this handler directly.
inline_try(Size) ->
    try <<"":Size>> of B -> {built, B} catch error:Reason -> {caught, Reason} end.

%% The try is in the same function as each construction, so a pending
%% exception reaches this handler directly. Z is 0 for the failing cases.
arith_try(Kind, Z) ->
    X = id(120), B = id(<<"xy">>),
    try
        case Kind of
            string -> <<"ab":(16 div Z)>>;
            string_float -> <<"ab":(32 div Z)/float>>;
            string_binary -> <<"ab":(1 div Z)/binary>>;
            empty -> <<"":(1 div Z)>>;
            empty_binary -> <<"":(1 div Z)/binary>>;
            integer -> <<X:(8 div Z)>>;
            float -> <<X:(64 div Z)/float>>;
            binary -> <<B:(2 div Z)/binary>>;
            binary_value -> <<(1 div Z)/binary>>
        end
    of Built -> {built, Built}
    catch error:Reason -> {caught, Reason}
    end.

size_once() ->
    put(size_calls, 0),
    B = <<"abc":(next_size())>>,
    {B, get(size_calls)}.

next_size() ->
    put(size_calls, get(size_calls) + 1),
    16.

guard_utf16(B) when B =:= <<"λ"/utf16>> -> yes;
guard_utf16(_) -> no.

guard_size(B, N) when B =:= <<"é":N>> -> yes;
guard_size(_, _) -> no.

native(Big, Little) ->
    case <<1:16/native>> of
        <<0,1>> -> Big;
        <<1,0>> -> Little
    end.

id(X) -> X.
