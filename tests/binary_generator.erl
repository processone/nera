%% EXPECT: binary_generator_ok
%% NERA-48: <= generators consume whole patterns, not individual bytes.
%% Run this same fixture with nera test and erlc/erl; both must reach the marker.
-module(binary_generator).
-export([test/0]).

test() ->
    lists:foreach(fun({Kind, Input, ExpectedList, ExpectedBinary}) ->
        ExpectedList = lc(Kind, id(Input)),
        ExpectedBinary = bc(Kind, id(Input)),
        io:format("~w: ~w ~w~n", [Kind, ExpectedList, ExpectedBinary])
    end, cases()),
    nested(),
    cursor_lifetime(),
    utf_cursors(),
    input_scope(),
    errors(),
    io:format("binary_generator_ok~n"),
    ok.

cases() ->
    [{byte, <<1,2,3>>, [1,2,3], <<1,2,3>>},
     {byte, <<>>, [], <<>>},
     {byte, <<1,2,3:2>>, [1,2], <<1,2>>},
     {word, <<1,2,3,4>>, [258,772], <<1,2,3,4>>},
     {word, <<1,2,3>>, [258], <<1,2>>},
     {word, <<1>>, [], <<>>},
     {nibble, <<16#12,3:4>>, [1,2,3], <<1,2,3>>},
     {nibble, <<1:2>>, [], <<>>},
     {pair, <<1,2,3,4>>, [{1,2},{3,4}], <<2,1,4,3>>},
     {pair, <<1,2,3>>, [{1,2}], <<2,1>>},
     {wide, <<16#abc:12,16#def:12,7:3>>, [16#abc,16#def], <<16#abc:16,16#def:16>>},
     {signed, <<255,127>>, [-1,127], <<255,127>>},
     {little, <<1,2,3,4>>, [513,1027], <<2,1,4,3>>},
     {unit, <<1,2,3,4>>, [258,772], <<1,2,3,4>>},
     {chunk, <<1,2,3,4,5>>, [<<1,2>>,<<3,4>>], <<1,2,3,4>>},
     {bits, <<5:3,6:3,7:3,1:1>>, [<<5:3>>,<<6:3>>,<<7:3>>], <<5,6,7>>},
     {literal, <<0,9,1,2,0,8,1,4,1>>, [2,4], <<2,4>>},
     {repeated, <<1,1,2,3,4,4,5>>, [1,4], <<1,4>>},
     {repeated_chunk, <<1,1,2,3,4,4>>, [<<1>>,<<4>>], <<1,4>>},
     {string, <<"ax9abzabq">>, [$z,$q], <<"zq">>},
     {float, <<1.5:64/float,2.5:64/float,0:7>>, [1.5,2.5], <<1.5:64/float,2.5:64/float>>},
     %% NERA-73 F2: nonfinite fields skip one full element, even before more fields.
     {float, <<16#7ff0000000000000:64,1.5:64/float,16#7ff8000000000000:64,2.5:64/float>>,
      [1.5,2.5], <<1.5:64/float,2.5:64/float>>},
     {float_pair, <<16#7ff0000000000000:64,9,1.5:64/float,7,16#fff0000000000000:64,8,2.5:64/float,6>>,
      [{1.5,7},{2.5,6}], <<1.5:64/float,7,2.5:64/float,6>>},
     {float_wildcard, <<16#7ff8000000000000:64,9,1.5:64/float,7>>, [7], <<7>>},
     {float_two, <<16#7ff0000000000000:64,1.5:64/float,1.5:64/float,16#7ff0000000000000:64,1.5:64/float,2.5:64/float>>,
      [{1.5,2.5}], <<1.5:64/float,2.5:64/float>>},
     {float16, <<16#7c00:16,1.5:16/float,16#7e00:16,2.5:16/float>>, [1.5,2.5], <<1.5:64/float,2.5:64/float>>},
     {float32, <<16#7f800000:32,1.5:32/float,16#7fc00000:32,2.5:32/float>>, [1.5,2.5], <<1.5:64/float,2.5:64/float>>},
     {float_little, <<16#7ff0000000000000:64/little,1.5:64/float-little>>, [1.5], <<1.5:64/float>>},
     %% NERA-103 F2: float widths scale by the unit, in generators and construction.
     {float_unit, <<1.5:1/float-unit:64,2.5:2/float-unit:32,0:3>>, [1.5,2.5], <<1.5:64/float,2.5:64/float>>},
     {utf8, <<65,16#20ac/utf8,66>>, [65,16#20ac,66], <<65/utf8,16#20ac/utf8,66/utf8>>},
     {utf8, <<65,255,66>>, [65], <<65>>},
     {utf16, <<65/utf16-little,16#1f600/utf16-little,0:7>>, [65,16#1f600], <<65/utf8,16#1f600/utf8>>},
     {utf32, <<65/utf32,16#20ac/utf32,0:8>>, [65,16#20ac], <<65/utf8,16#20ac/utf8>>},
     {utf8_stride, <<0:1,65/utf8,0:1,16#20ac/utf8,0:1,66/utf8>>, [65,16#20ac,66], <<65/utf8,16#20ac/utf8,66/utf8>>},
     {utf16_stride, <<0:1,65/utf16-little,0:1,16#1f600/utf16-little>>, [65,16#1f600], <<65/utf8,16#1f600/utf8>>},
     {utf32_stride, <<0:1,65/utf32,0:1,16#20ac/utf32>>, [65,16#20ac], <<65/utf8,16#20ac/utf8>>},
     {length, <<1,7,2,1,2,1,9,3,0>>, [7,258,9], <<7:16,258:16,9:16>>},
     {filtered, <<1,2,3,4,5>>, [2,4], <<2,4>>}].

lc(byte, B) -> [X || <<X>> <= B];
lc(word, B) -> [X || <<X:16>> <= B];
lc(nibble, B) -> [X || <<X:4>> <= B];
lc(pair, B) -> [{X,Y} || <<X:8,Y:8>> <= B];
lc(wide, B) -> [X || <<X:12>> <= B];
lc(signed, B) -> [X || <<X:8/signed>> <= B];
lc(little, B) -> [X || <<X:16/little>> <= B];
lc(unit, B) -> [X || <<X:2/unit:8>> <= B];
lc(chunk, B) -> [X || <<X:2/binary>> <= B];
lc(bits, B) -> [X || <<X:3/bits>> <= B];
lc(literal, B) -> [X || <<1:8,X:8>> <= B];
lc(repeated, B) -> [X || <<X:8,X:8>> <= B];
lc(repeated_chunk, B) -> [X || <<X:1/binary,X:1/binary>> <= B];
lc(string, B) -> [X || <<"ab",X>> <= B];
lc(float, B) -> [X || <<X:64/float>> <= B];
lc(float_pair, B) -> [{X,Y} || <<X:64/float,Y>> <= B];
lc(float_two, B) -> [{X,Y} || <<X:64/float,Y:64/float>> <= B];
lc(float_wildcard, B) -> [Y || <<_:64/float,Y>> <= B];
lc(float16, B) -> [X || <<X:16/float>> <= B];
lc(float32, B) -> [X || <<X:32/float>> <= B];
lc(float_little, B) -> [X || <<X:64/float-little>> <= B];
lc(float_unit, B) -> [X || <<X:1/float-unit:64>> <= B];
lc(utf8, B) -> [X || <<X/utf8>> <= B];
lc(utf16, B) -> [X || <<X/utf16-little>> <= B];
lc(utf32, B) -> [X || <<X/utf32>> <= B];
lc(utf8_stride, B) -> [X || <<_:1,X/utf8>> <= B];
lc(utf16_stride, B) -> [X || <<_:1,X/utf16-little>> <= B];
lc(utf32_stride, B) -> [X || <<_:1,X/utf32>> <= B];
lc(length, B) -> [X || <<N:8,X:N/unit:8>> <= B];
lc(filtered, B) -> [X || <<X>> <= B, X rem 2 =:= 0].

bc(byte, B) -> << <<X>> || <<X>> <= B >>;
bc(word, B) -> << <<X:16>> || <<X:16>> <= B >>;
bc(nibble, B) -> << <<X>> || <<X:4>> <= B >>;
bc(pair, B) -> << <<Y,X>> || <<X:8,Y:8>> <= B >>;
bc(wide, B) -> << <<X:16>> || <<X:12>> <= B >>;
bc(signed, B) -> << <<X:8/signed>> || <<X:8/signed>> <= B >>;
bc(little, B) -> << <<X:16>> || <<X:16/little>> <= B >>;
bc(unit, B) -> << <<X:16>> || <<X:2/unit:8>> <= B >>;
bc(chunk, B) -> << X || <<X:2/binary>> <= B >>;
bc(bits, B) -> << <<X:8>> || <<X:3>> <= B >>;
bc(literal, B) -> << <<X>> || <<1:8,X:8>> <= B >>;
bc(repeated, B) -> << <<X>> || <<X:8,X:8>> <= B >>;
bc(repeated_chunk, B) -> << X || <<X:1/binary,X:1/binary>> <= B >>;
bc(string, B) -> << <<X>> || <<"ab",X>> <= B >>;
bc(float, B) -> << <<X:64/float>> || <<X:64/float>> <= B >>;
bc(float_pair, B) -> << <<X:64/float,Y>> || <<X:64/float,Y>> <= B >>;
bc(float_two, B) -> << <<X:64/float,Y:64/float>> || <<X:64/float,Y:64/float>> <= B >>;
bc(float_wildcard, B) -> << <<Y>> || <<_:64/float,Y>> <= B >>;
bc(float16, B) -> << <<X:64/float>> || <<X:16/float>> <= B >>;
bc(float32, B) -> << <<X:64/float>> || <<X:32/float>> <= B >>;
bc(float_little, B) -> << <<X:64/float>> || <<X:64/float-little>> <= B >>;
bc(float_unit, B) -> << <<X:4/float-unit:16>> || <<X:2/float-unit:32>> <= B >>;
bc(utf8, B) -> << <<X/utf8>> || <<X/utf8>> <= B >>;
bc(utf16, B) -> << <<X/utf8>> || <<X/utf16-little>> <= B >>;
bc(utf32, B) -> << <<X/utf8>> || <<X/utf32>> <= B >>;
bc(utf8_stride, B) -> << <<X/utf8>> || <<_:1,X/utf8>> <= B >>;
bc(utf16_stride, B) -> << <<X/utf8>> || <<_:1,X/utf16-little>> <= B >>;
bc(utf32_stride, B) -> << <<X/utf8>> || <<_:1,X/utf32>> <= B >>;
bc(length, B) -> << <<X:16>> || <<N:8,X:N/unit:8>> <= B >>;
bc(filtered, B) -> << <<X>> || <<X>> <= B, X rem 2 =:= 0 >>.

nested() ->
    B = id(<<1,2,3,4>>),
    mixed_lists(B, id([10,20])),
    mixed_binaries(B, id([10,20])),
    filtered_nested(B),
    shadowed_chunks(B),
    earlier_sizes(),
    nested_sources(B),
    shadowed_names(B),
    shifted_source(),
    io:format("nested_ok~n").

mixed_lists(B, L) ->
    [{1,10},{1,20},{3,10},{3,20}] = [{X,Y} || <<X:8,_:8>> <= B, Y <- L],
    [{10,1},{10,3},{20,1},{20,3}] = [{Y,X} || Y <- L, <<X:8,_:8>> <= B].

mixed_binaries(B, L) ->
    <<10,1,10,3,20,1,20,3>> = << <<Y,X>> || Y <- L, <<X:8,_:8>> <= B >>,
    <<1,10,1,20,3,10,3,20>> = << <<X,Y>> || <<X:8,_:8>> <= B, Y <- L >>.

filtered_nested(B) ->
    [{2,1},{2,2},{4,1},{4,2}] = [{X,Y} || <<X>> <= B, X rem 2 =:= 0, <<Y:4>> <= <<16#12>>],
    <<2,1,2,2,4,1,4,2>> = << <<X,Y>> || <<X>> <= B, X rem 2 =:= 0, <<Y:4>> <= <<16#12>> >>.

shadowed_chunks(B) ->
    [<<1,2>>,<<3,4>>,<<1,2>>,<<3,4>>] = [X || <<X:2/binary>> <= B, <<X:2/binary>> <= B],
    <<1,2,3,4,1,2,3,4>> = << X || <<X:2/binary>> <= B, <<X:2/binary>> <= B >>.

earlier_sizes() ->
    [<<1>>,<<2>>,<<1,2>>] = [X || N <- id([1,2]), <<X:N/binary>> <= id(<<1,2>>)],
    <<1,2,1,2>> = << X || N <- id([1,2]), <<X:N/binary>> <= id(<<1,2>>) >>,
    [<<1>>,<<2>>,<<1,2>>] = [X || <<N>> <= id(<<1,2>>), <<X:N/binary>> <= id(<<1,2>>)],
    <<1,2,1,2>> = << X || <<N>> <= id(<<1,2>>), <<X:N/binary>> <= id(<<1,2>>) >>.

nested_sources(B) ->
    [1,2,3,4] = [Y || <<X:2/binary>> <= B, <<Y>> <= X],
    <<1,2,3,4>> = << <<Y>> || <<X:2/binary>> <= B, <<Y>> <= X >>.

shadowed_names(B) ->
    [9,9,9,9] = [X || <<X>> <= B, X <- id([9])],
    <<9,9,9,9>> = << <<X>> || <<X>> <= B, X <- id([9]) >>,
    [99,99,99,99] = [(fun() -> X end)() || <<X>> <= B, X <- id([99])],
    {[1,2,3,4], <<1,2,3,4>>, 99} = outer_shadow(id(99), B).

shifted_source() ->
    <<_:3, Shifted/bits>> = id(<<7:3,16#abc:12,16#def:12>>),
    [16#abc,16#def] = lc(wide, Shifted),
    <<16#abc:16,16#def:16>> = bc(wide, Shifted).

outer_shadow(X, B) ->
    {[X || <<X>> <= B], << <<X>> || <<X>> <= B >>, X}.

%% NERA-73 F1: the cursor must survive allocation and nested continuations,
%% without changing the source or an earlier generator's retained value.
cursor_lifetime() ->
    B = id(binary:copy(<<16#12>>, 256)),
    Values = [X || <<X:4>> <= B, _ <- lists:seq(1,20)],
    10240 = length(Values),
    15360 = lists:sum(Values),
    B = binary:copy(<<16#12>>, 256),
    Parts = [X || <<X:1/bits>> <= id(<<1:1,0:1,1:1>>), _ <- lists:seq(1,500)],
    1500 = length(Parts),
    <<1:1>> = hd(Parts),
    io:format("cursor_lifetime_ok~n").

%% The same raw UTF readers serve generator cursors and ordinary recursion.
utf_cursors() ->
    [65,8364] = utf_loop(utf8, id(<<0:1,65/utf8,0:1,16#20ac/utf8>>), []),
    [65,128512] = utf_loop(utf16, id(<<0:1,65/utf16-little,0:1,16#1f600/utf16-little>>), []),
    [65,8364] = utf_loop(utf32, id(<<0:1,65/utf32,0:1,16#20ac/utf32>>), []),
    io:format("utf_cursors_ok~n").
utf_loop(_, <<>>, A) -> lists:reverse(A);
utf_loop(utf8, <<_:1,X/utf8,T/bits>>, A) -> utf_loop(utf8, T, [X|A]);
utf_loop(utf16, <<_:1,X/utf16-little,T/bits>>, A) -> utf_loop(utf16, T, [X|A]);
utf_loop(utf32, <<_:1,X/utf32,T/bits>>, A) -> utf_loop(utf32, T, [X|A]).

%% PR #131 integration: inputs are captured before shadowing, while the
%% prefix matcher writes separate destination placeholders for its values.
input_scope() ->
    B = id(<<1,2,3,4>>),
    {[258,772], <<1,2,3,4>>} = outer_size(id(16), B),
    [{1,2},{3,4}] = size_shadow(id(16), id(<<1:16,2,3:16,4>>)),
    <<1:16,2,3:16,4>> = size_shadow_binary(id(16), id(<<1:16,2,3:16,4>>)),
    [258,772] = tail_size(id(<<0,7,8>>), B),
    io:format("input_scope_ok~n").
outer_size(N, B) -> {[X || <<X:N>> <= B], << <<X:N>> || <<X:N>> <= B >>}.
size_shadow(N, B) -> [{X,N} || <<X:N,N:8>> <= B].
size_shadow_binary(N, B) -> << <<X:16,N>> || <<X:N,N:8>> <= B >>.
tail_size(<<_:8,Key/binary>>, B) -> [X || <<X:(byte_size(Key)*8)>> <= B].

errors() ->
    lists:foreach(fun(B) ->
        {error,{bad_generator,B}} = result(fun() -> lc(byte, B) end),
        {error,{bad_generator,B}} = result(fun() -> bc(byte, B) end)
    end, [not_binary, [1,2], 7]),
    {error,{bad_generator,not_binary}} = result(fun() ->
        [X || N <- id([1]), N > 0, <<X>> <= id(not_binary)]
    end),
    {error,{bad_generator,not_binary}} = result(fun() ->
        << <<X>> || N <- id([1]), N > 0, <<X>> <= id(not_binary) >>
    end),
    io:format("errors_ok~n").

result(F) -> try F() catch Class:Reason -> {Class,Reason} end.
id(X) -> X.
