-module(binary_unicode_bits).
-export([test/0]).

test() ->
    lists:foreach(fun(C) ->
        bad(fun() -> put(utf8, C, 0) end),
        bad(fun() -> put(utf16, C, 0) end),
        bad(fun() -> put(utf32, C, 0) end)
    end, [-1, 16#D800, 16#DFFF, 16#110000, 4294967361,
          1152921504606846975, 1208925819614629174706176, 65.0, invalid, []]),
    caught = try <<(id(4294967361))/utf8>>, error(continued)
             catch error:badarg -> caught end,
    caught = try <<(id(16#D800))/utf16>>, error(continued)
             catch error:badarg -> caught end,
    caught = try <<(id(16#110000))/utf32>>, error(continued)
             catch error:badarg -> caught end,
    lists:foreach(fun({C, U8, U16, U16LE, U32, U32LE}) ->
        lists:foreach(fun(P) ->
            lists:foreach(fun({Kind, Bytes}) ->
                Expected = <<1:P, Bytes/binary, 3:2>>,
                Expected = put(Kind, C, P),
                {C, 3} = get(Kind, Expected, P),
                %% Force a refc backing allocation and a nonzero byte origin.
                Large = <<0:1024, Expected/bitstring>>,
                <<_:1024, Rest/bitstring>> = Large,
                {C, 3} = get(Kind, Rest, P)
            end, [{utf8,U8}, {utf16,U16}, {utf16le,U16LE},
                  {utf32,U32}, {utf32le,U32LE}])
        end, lists:seq(0,7))
    end, [{0, <<0>>, <<0,0>>, <<0,0>>, <<0,0,0,0>>, <<0,0,0,0>>},
          {65, <<65>>, <<0,65>>, <<65,0>>, <<0,0,0,65>>, <<65,0,0,0>>},
          {16#20AC, <<226,130,172>>, <<32,172>>, <<172,32>>,
                   <<0,0,32,172>>, <<172,32,0,0>>},
          {16#1F600, <<240,159,152,128>>, <<216,61,222,0>>, <<61,216,0,222>>,
                     <<0,1,246,0>>, <<0,246,1,0>>},
          {16#10FFFF, <<244,143,191,191>>, <<219,255,223,255>>, <<255,219,255,223>>,
                      <<0,16,255,255>>, <<255,255,16,0>>}]),
    lists:foreach(fun(P) ->
        lists:foreach(fun({Kind, Bytes}) ->
            fail = decode(Kind, <<1:P, Bytes/binary>>, P)
        end, [{utf8, <<192,128>>}, {utf8, <<237,160,128>>},
              {utf8, <<244,144,128,128>>}, {utf8, <<240,159,152>>},
              {utf16, <<216,0>>}, {utf16, <<220,0>>}, {utf16, <<216,0,0,65>>},
              {utf32, <<0,0,216,0>>}, {utf32, <<0,17,0,0>>}]),
        fail = decode(utf8, <<1:P, 65:7>>, P),
        fail = decode(utf16, <<1:P, 65:15>>, P),
        fail = decode(utf32, <<1:P, 65:31>>, P)
    end, lists:seq(0,7)),
    ok.

put(utf8, C, P) -> <<1:P, C/utf8, 3:2>>;
put(utf16, C, P) -> <<1:P, C/utf16, 3:2>>;
put(utf16le, C, P) -> <<1:P, C/utf16-little, 3:2>>;
put(utf32, C, P) -> <<1:P, C/utf32, 3:2>>;
put(utf32le, C, P) -> <<1:P, C/utf32-little, 3:2>>.
get(utf8, Bin, P) -> <<_:P, C/utf8, T:2>> = Bin, {C,T};
get(utf16, Bin, P) -> <<_:P, C/utf16, T:2>> = Bin, {C,T};
get(utf16le, Bin, P) -> <<_:P, C/utf16-little, T:2>> = Bin, {C,T};
get(utf32, Bin, P) -> <<_:P, C/utf32, T:2>> = Bin, {C,T};
get(utf32le, Bin, P) -> <<_:P, C/utf32-little, T:2>> = Bin, {C,T}.
decode(utf8, Bin, P) -> case Bin of <<_:P, C/utf8>> -> C; _ -> fail end;
decode(utf16, Bin, P) -> case Bin of <<_:P, C/utf16>> -> C; _ -> fail end;
decode(utf32, Bin, P) -> case Bin of <<_:P, C/utf32>> -> C; _ -> fail end.
id(X) -> X.
bad(F) -> {'EXIT', {badarg, _}} = (catch F()), ok.
