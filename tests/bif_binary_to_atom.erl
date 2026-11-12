%% EXPECT: bif_binary_to_atom_ok
%% Regression: Cowboy 2.20.0 cowboy_router calls the auto-imported
%% binary_to_atom/2. Every expected value and error below matches OTP 29.
-module(bif_binary_to_atom).
-export([test/0]).

test() ->
    %% Unqualified and qualified calls with each accepted encoding.
    abc = binary_to_atom(id(<<"abc">>), utf8),
    abc = binary_to_atom(id(<<"abc">>), unicode),
    abc = binary_to_atom(id(<<"abc">>), latin1),
    abc = erlang:binary_to_atom(id(<<"abc">>), id(utf8)),
    '' = binary_to_atom(id(<<>>), utf8),
    id = route_binding(<<":id">>),
    %% utf8 decodes characters; latin1 maps each byte to one character.
    [1024] = atom_to_list(binary_to_atom(id(<<1024/utf8>>), utf8)),
    [233] = atom_to_list(binary_to_atom(id(<<233>>), latin1)),
    [195, 169] = atom_to_list(binary_to_atom(id(<<195, 169>>), latin1)),
    [0, 255] = atom_to_list(binary_to_atom(id(<<0, 255>>), latin1)),
    true = binary_to_atom(id(<<195, 169>>), utf8)
        =:= binary_to_atom(id(<<233>>), latin1),
    %% Sub-binaries: a small copy and a shared slice at a nonzero offset.
    <<_, Small:3/binary, _/binary>> = id(<<"xkeyz">>),
    key = binary_to_atom(Small, utf8),
    Large = id(<<"prefix", (binary:copy(<<"k">>, 100))/binary, 233>>),
    <<"prefix", Shared:100/binary, LastByte/binary>> = Large,
    true = binary:referenced_byte_size(Shared) > byte_size(Shared),
    SharedName = list_to_atom(lists:duplicate(100, $k)),
    SharedName = binary_to_atom(Shared, utf8),
    SharedName = binary_to_atom(Shared, latin1),
    [233] = atom_to_list(binary_to_atom(LastByte, latin1)),
    %% binary_to_atom/1 behaves as binary_to_atom/2 with utf8.
    [1024] = atom_to_list(binary_to_atom(id(<<1024/utf8>>))),
    %% badarg: invalid UTF-8, unknown encoding, non-binary or bitstring.
    badarg = reason(fun() -> binary_to_atom(id(<<233>>), utf8) end),
    badarg = reason(fun() -> binary_to_atom(id(<<233>>), unicode) end),
    badarg = reason(fun() -> binary_to_atom(id(<<237, 160, 128>>), utf8) end),
    badarg = reason(fun() -> binary_to_atom(id(<<192, 128>>), utf8) end),
    badarg = reason(fun() -> binary_to_atom(id(<<"abc">>), id(utf16)) end),
    badarg = reason(fun() -> binary_to_atom(id(<<"abc">>), id(<<"utf8">>)) end),
    badarg = reason(fun() -> binary_to_atom(id(abc), utf8) end),
    badarg = reason(fun() -> binary_to_atom(id("abc"), latin1) end),
    badarg = reason(fun() -> binary_to_atom(id(<<"a", 1:3>>), latin1) end),
    badarg = reason(fun() -> binary_to_atom(id(<<"a", 1:3>>)) end),
    badarg = reason(fun() -> binary_to_atom(id(<<233>>)) end),
    %% Atom names are limited to 255 characters, not bytes.
    255 = length(atom_to_list(binary_to_atom(chars(<<"a">>, 255), utf8))),
    255 = length(atom_to_list(binary_to_atom(chars(<<1024/utf8>>, 255), utf8))),
    255 = length(atom_to_list(binary_to_atom(chars(<<233>>, 255), latin1))),
    255 = length(atom_to_list(binary_to_atom(chars(<<16#10000/utf8>>, 255), utf8))),
    system_limit = reason(fun() -> binary_to_atom(chars(<<"a">>, 256), utf8) end),
    system_limit = reason(fun() -> binary_to_atom(chars(<<1024/utf8>>, 256), unicode) end),
    system_limit = reason(fun() -> binary_to_atom(chars(<<233>>, 256), latin1) end),
    system_limit = reason(fun() -> binary_to_atom(chars(<<"a">>, 256)) end),
    %% Invalid UTF-8 up to 4 * 255 bytes is badarg; a bad encoding or a
    %% bitstring is badarg at any size.
    badarg = reason(fun() -> binary_to_atom(chars(<<233>>, 256), utf8) end),
    badarg = reason(fun() -> binary_to_atom(chars(<<233>>, 1020), utf8) end),
    badarg = reason(fun() -> binary_to_atom(chars(<<"a">>, 256), id(utf16)) end),
    %% Longer input raises system_limit before it is decoded or validated,
    %% including a large binary.
    system_limit = reason(fun() -> binary_to_atom(chars(<<233>>, 1021), utf8) end),
    Huge = chars(<<233>>, 16#1000000),
    system_limit = reason(fun() -> binary_to_atom(Huge, latin1) end),
    system_limit = reason(fun() -> binary_to_atom(Huge, utf8) end),
    system_limit = reason(fun() -> binary_to_atom(Huge, unicode) end),
    system_limit = reason(fun() -> binary_to_atom(Huge) end),
    badarg = reason(fun() -> binary_to_atom(Huge, id(utf16)) end),
    badarg = reason(fun() -> binary_to_atom(<<Huge/binary, 1:1>>, latin1) end),
    %% Dynamic calls reach the same implementation.
    xyz = erlang:apply(erlang, binary_to_atom, [id(<<"xyz">>), utf8]),
    Convert = fun erlang:binary_to_atom/2,
    [233] = atom_to_list(Convert(id(<<233>>), latin1)),
    badarg = reason(fun() -> Convert(id(<<"abc">>), id(utf16)) end),
    io:format("bif_binary_to_atom_ok~n"),
    ok.

%% Shape of cowboy_router:compile_rules/5 for a ":name" path segment.
route_binding(<<$:, NameBin/binary>>) ->
    binary_to_atom(NameBin, utf8).

reason(Fun) ->
    try Fun() of
        Value -> {returned, Value}
    catch
        error:Reason -> Reason
    end.

chars(Char, Count) -> id(binary:copy(Char, Count)).

id(Value) -> Value.
