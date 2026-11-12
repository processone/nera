%% EXPECT: bif_atom_encoding_ok
%% NERA-40: binary_to_existing_atom/2 and atom_to_binary/2 must honor
%% latin1, utf8 and unicode, and reject other encodings like BEAM OTP 29.
-module(bif_atom_encoding).
-export([test/0]).

test() ->
    encodings([latin1, utf8, unicode]),
    %% All Latin-1 bytes, including NUL and 255, survive both conversions.
    %% Split the range so each atom stays below OTP's 255-character limit.
    latin1_roundtrip(lists:seq(0, 127)),
    latin1_roundtrip(lists:seq(128, 255)),
    Accent = list_to_atom([233]),
    Accent = binary_to_existing_atom(id(<<233>>), latin1),
    <<233>> = atom_to_binary(id(Accent), latin1),
    Accent = binary_to_existing_atom(id(<<195, 169>>), utf8),
    Accent = binary_to_existing_atom(id(<<195, 169>>), unicode),
    <<195, 169>> = atom_to_binary(id(Accent), utf8),
    <<195, 169>> = atom_to_binary(id(Accent), unicode),
    TwoChars = list_to_atom([195, 169]),
    TwoChars = binary_to_existing_atom(id(<<195, 169>>), latin1),
    Wide = list_to_atom([256, 1024, 16#10000, 16#10FFFF]),
    WideBin = id(<<256/utf8, 1024/utf8, 16#10000/utf8, 16#10FFFF/utf8>>),
    Wide = binary_to_existing_atom(WideBin, unicode),
    WideBin = atom_to_binary(id(Wide), utf8),
    badarg = reason(fun() -> atom_to_binary(id(Wide), latin1) end),
    %% /1 keeps its UTF-8 default, not a dummy /2 encoding.
    Wide = binary_to_existing_atom(WideBin),
    WideBin = atom_to_binary(id(Wide)),
    badarg = reason(fun() -> binary_to_existing_atom(id(<<233>>)) end),
    invalid_encodings([utf16, utf32, unknown_encoding, <<"utf8">>, "utf8", 0, [], {utf8}]),
    invalid_inputs([ok, "ok", 42, [], {ok}]),
    invalid_utf8([<<233>>, <<192, 128>>, <<237, 160, 128>>, <<244, 144, 128, 128>>]),
    %% Failed lookup must not intern a new atom. Both calls must raise badarg.
    Missing = id(<<"nera40_missing_atom_761c189f">>),
    badarg = reason(fun() -> binary_to_existing_atom(Missing, latin1) end),
    badarg = reason(fun() -> binary_to_existing_atom(Missing, utf8) end),
    badarg = reason(fun() -> binary_to_existing_atom(Missing) end),
    sub_binaries(Accent),
    limits([latin1, utf8, unicode]),
    dynamic_calls(Accent),
    io:format("bif_atom_encoding_ok~n"),
    ok.

encodings([]) -> ok;
encodings([Encoding | Rest]) ->
    ok = binary_to_existing_atom(id(<<"ok">>), id(Encoding)),
    <<"ok">> = atom_to_binary(id(ok), id(Encoding)),
    '' = binary_to_existing_atom(id(<<>>), id(Encoding)),
    <<>> = atom_to_binary(id(''), id(Encoding)),
    encodings(Rest).

latin1_roundtrip(Chars) ->
    Atom = list_to_atom(Chars),
    Bin = list_to_binary(Chars),
    Atom = binary_to_existing_atom(id(Bin), latin1),
    Bin = atom_to_binary(id(Atom), latin1),
    ok.

invalid_encodings([]) -> ok;
invalid_encodings([Encoding | Rest]) ->
    badarg = reason(fun() -> binary_to_existing_atom(id(<<"ok">>), id(Encoding)) end),
    badarg = reason(fun() -> atom_to_binary(id(ok), id(Encoding)) end),
    invalid_encodings(Rest).

invalid_inputs([]) -> ok;
invalid_inputs([Value | Rest]) ->
    badarg = reason(fun() -> binary_to_existing_atom(id(Value), latin1) end),
    invalid_inputs(Rest).

invalid_utf8([]) -> ok;
invalid_utf8([Bin | Rest]) ->
    badarg = reason(fun() -> binary_to_existing_atom(id(Bin), utf8) end),
    badarg = reason(fun() -> binary_to_existing_atom(id(Bin), unicode) end),
    invalid_utf8(Rest).

sub_binaries(Accent) ->
    %% A small slice, a shared refc slice and an unaligned whole-byte slice.
    <<_, Small:1/binary, _/binary>> = id(<<0, 233, 0>>),
    Accent = binary_to_existing_atom(Small, latin1),
    Name = list_to_atom(lists:duplicate(100, 233)),
    Parent = id(<<0, (binary:copy(<<233>>, 100))/binary, 0>>),
    <<_, Shared:100/binary, _/binary>> = Parent,
    true = binary:referenced_byte_size(Shared) > byte_size(Shared),
    Name = binary_to_existing_atom(Shared, latin1),
    ShiftedParent = id(<<0:3, 233, 0:5>>),
    <<_:3, Shifted:1/binary, _:5>> = ShiftedParent,
    Accent = binary_to_existing_atom(Shifted, latin1),
    %% Partial bitstrings must not be rounded to an existing name.
    %% The old runtime rounded <<"ok", 0:1>> up to the atom "ok\0".
    _ = list_to_atom([$o, $k, 0]),
    badarg = reason(fun() -> binary_to_existing_atom(id(<<"ok", 0:1>>), latin1) end),
    badarg = reason(fun() -> binary_to_existing_atom(id(<<"ok", 0:1>>)) end),
    Partial = id(<<Parent/binary, 0:1>>),
    badarg = reason(fun() -> binary_to_existing_atom(Partial, utf8) end),
    ok.

limits([]) -> ok;
limits([Encoding | Rest]) ->
    %% Valid names are limited by characters, not UTF-8 byte count.
    {Char, Bytes} = case Encoding of
        latin1 -> {233, <<233>>};
        _ -> {16#10000, <<16#10000/utf8>>}
    end,
    Max = list_to_atom(lists:duplicate(255, Char)),
    MaxBin = id(binary:copy(Bytes, 255)),
    Max = binary_to_existing_atom(MaxBin, Encoding),
    MaxBin = atom_to_binary(id(Max), Encoding),
    badarg = reason(fun() -> binary_to_existing_atom(id(binary:copy(Bytes, 256)), Encoding) end),
    %% Existing-atom lookup uses badarg, never binary_to_atom's system_limit.
    badarg = reason(fun() -> binary_to_existing_atom(id(binary:copy(<<"a">>, 256)), Encoding) end),
    badarg = reason(fun() -> binary_to_existing_atom(id(binary:copy(<<233>>, 1021)), Encoding) end),
    limits(Rest).

dynamic_calls(Accent) ->
    %% Qualified calls, dynamic apply and external funs share the same contract.
    Accent = erlang:binary_to_existing_atom(id(<<233>>), id(latin1)),
    <<233>> = erlang:atom_to_binary(id(Accent), id(latin1)),
    Accent = erlang:apply(id(erlang), id(binary_to_existing_atom), [id(<<233>>), latin1]),
    <<233>> = erlang:apply(id(erlang), id(atom_to_binary), [id(Accent), latin1]),
    Existing = fun erlang:binary_to_existing_atom/2,
    ToBinary = fun erlang:atom_to_binary/2,
    Accent = Existing(id(<<233>>), latin1),
    <<233>> = ToBinary(id(Accent), latin1),
    badarg = reason(fun() -> Existing(id(<<"ok">>), id(utf16)) end),
    badarg = reason(fun() -> ToBinary(id(ok), id(utf16)) end),
    badarg = reason(fun() -> atom_to_binary(id(42), utf8) end),
    badarg = reason(fun() -> atom_to_binary(id(<<"ok">>), latin1) end),
    ok.

reason(Fun) ->
    try Fun() of
        Value -> {returned, Value}
    catch
        error:Reason -> Reason
    end.

id(Value) -> Value.
