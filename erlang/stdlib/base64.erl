%% Project-owned Base64 implementation. No OTP or Cowlib source is vendored.
%% Binary results; standard/URL alphabets and optional trailing padding.
-module(base64).
-export([encode/1, encode/2, decode/1, decode/2]).

%% OTP defaults: standard alphabet, mandatory padding, binary results.
encode(Data) -> encode(Data, #{}).
decode(Data) -> decode(Data, #{}).

encode(Data, Options) when is_map(Options), is_binary(Data) ->
    Mode = maps:get(mode, Options, standard),
    Padding = maps:get(padding, Options, true) =/= false,
    base64:encode_binary(Data, Mode =:= urlsafe, Padding);
encode(Data, Options) when is_map(Options), is_list(Data) ->
    encode(byte_list(Data), Options).

decode(Data, Options) when is_map(Options), is_binary(Data) ->
    Mode = maps:get(mode, Options, standard),
    Padding = maps:get(padding, Options, true) =/= false,
    base64:decode_binary(Data, Mode =:= urlsafe, Padding);
decode(Data, Options) when is_map(Options), is_list(Data) ->
    Mode = maps:get(mode, Options, standard),
    Padding = maps:get(padding, Options, true) =/= false,
    base64:decode_list(Data, Mode =:= urlsafe, Padding).

%% Validate flat byte lists, rather than accidentally accepting iodata.
byte_list(List) ->
    check_bytes(List),
    list_to_binary(List).
check_bytes([]) -> ok;
check_bytes([H|T]) when is_integer(H), H >= 0, H =< 255 -> check_bytes(T);
check_bytes([H|_]) when is_integer(H) -> error(badarg);
check_bytes([_|_]) -> error(badarith).
