%% Project-owned Base64 implementation. No OTP or Cowlib source is vendored.
%% Binary results; standard/URL alphabets and optional trailing padding.
-module(base64_reference).
-export([encode/2, decode/2]).

encode(Data, Options) when is_map(Options), is_binary(Data) ->
    Mode = maps:get(mode, Options, standard),
    Padding = maps:get(padding, Options, true) =/= false,
    iolist_to_binary(encode_bytes(Data, Mode, Padding, []));
encode(Data, Options) when is_map(Options), is_list(Data) ->
    encode(byte_list(Data), Options).

decode(Data, Options) when is_map(Options), is_binary(Data) ->
    Mode = maps:get(mode, Options, standard),
    Padding = maps:get(padding, Options, true) =/= false,
    decode_groups(symbols(binary_to_list(Data), Mode, []), Padding, []);
decode(Data, Options) when is_map(Options), is_list(Data) ->
    Mode = maps:get(mode, Options, standard),
    Padding = maps:get(padding, Options, true) =/= false,
    decode_groups(symbols(Data, Mode, []), Padding, []).

%% Validate flat byte lists, rather than accidentally accepting iodata.
byte_list(List) ->
    check_bytes(List),
    list_to_binary(List).
check_bytes([]) -> ok;
check_bytes([H|T]) when is_integer(H), H >= 0, H =< 255 -> check_bytes(T);
check_bytes([H|_]) when is_integer(H) -> error(badarg);
check_bytes([_|_]) -> error(badarith).

encode_bytes(<<A:6,B:6,C:6,D:6,Tail/binary>>, Mode, Pad, Acc) ->
    Chunk = <<(alphabet(A,Mode)),(alphabet(B,Mode)),(alphabet(C,Mode)),(alphabet(D,Mode))>>,
    encode_bytes(Tail, Mode, Pad, [Chunk|Acc]);
encode_bytes(<<A:6,B:6,C:4>>, Mode, Pad, Acc) ->
    Chunk = <<(alphabet(A,Mode)),(alphabet(B,Mode)),(alphabet(C bsl 2,Mode))>>,
    finish_encode(Chunk, <<"=">>, Pad, Acc);
encode_bytes(<<A:6,B:2>>, Mode, Pad, Acc) ->
    Chunk = <<(alphabet(A,Mode)),(alphabet(B bsl 4,Mode))>>,
    finish_encode(Chunk, <<"==">>, Pad, Acc);
encode_bytes(<<>>, _, _, Acc) -> lists:reverse(Acc).

finish_encode(Chunk, Suffix, true, Acc) -> lists:reverse([Suffix,Chunk|Acc]);
finish_encode(Chunk, _, false, Acc) -> lists:reverse([Chunk|Acc]).

alphabet(N, _) when N < 26 -> $A+N;
alphabet(N, _) when N < 52 -> $a+N-26;
alphabet(N, _) when N < 62 -> $0+N-52;
alphabet(62, urlsafe) -> $-;
alphabet(63, urlsafe) -> $_;
alphabet(62, _) -> $+;
alphabet(63, _) -> $/.

symbols([], _, Acc) -> lists:reverse(Acc);
symbols([C|Tail], Mode, Acc) when C =:= 32; C =:= 9; C =:= 10; C =:= 13 ->
    symbols(Tail, Mode, Acc);
symbols([C|Tail], Mode, Acc) -> symbols(Tail, Mode, [value(C,Mode)|Acc]).

value(C, _) when C >= $A, C =< $Z -> C-$A;
value(C, _) when C >= $a, C =< $z -> C-$a+26;
value(C, _) when C >= $0, C =< $9 -> C-$0+52;
value($-, urlsafe) -> 62;
value($_, urlsafe) -> 63;
value($+, Mode) when Mode =/= urlsafe -> 62;
value($/, Mode) when Mode =/= urlsafe -> 63;
value($=, _) -> eq;
value(C, _) when is_integer(C) -> bad;
value(_, _) -> error(badarith).

%% Validate before packing instead of relying on binary construction to reject
%% non-integer symbols. Padded tails perform the final shift first, preserving
%% BEAM's distinction between badarith and badarg.
decode_groups([A,B,eq,eq|Tail], _, Acc) ->
    Low = B bsr 4,
    Chunk = <<(checked6(A)):6,Low:2>>,
    finish_decode(Tail, [Chunk|Acc]);
decode_groups([A,B,C,eq|Tail], _, Acc) ->
    Low = C bsr 2,
    Chunk = <<(checked6(A)):6,(checked6(B)):6,Low:4>>,
    finish_decode(Tail, [Chunk|Acc]);
decode_groups([A,B,C,D|Tail], Pad, Acc) ->
    decode_groups(Tail, Pad, [<<(checked6(A)):6,(checked6(B)):6,(checked6(C)):6,(checked6(D)):6>>|Acc]);
decode_groups([], _, Acc) -> iolist_to_binary(lists:reverse(Acc));
decode_groups([A,B], false, Acc) -> decode_groups([A,B,eq,eq], false, Acc);
decode_groups([A,B,eq], false, Acc) -> decode_groups([A,B,eq,eq], false, Acc);
decode_groups([A,B,C], false, Acc) -> decode_groups([A,B,C,eq], false, Acc);
decode_groups([_,_], true, _) -> error(missing_padding);
decode_groups([_,_,_], true, _) -> error(missing_padding).

finish_decode([], Acc) -> iolist_to_binary(lists:reverse(Acc));
finish_decode([Symbol|_], _) -> error({case_clause,Symbol}).

checked6(N) when is_integer(N), N >= 0, N < 64 -> N;
checked6(_) -> error(badarg).
