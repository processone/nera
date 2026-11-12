-module(base64_default).
-export([test/0]).

test() ->
    Vectors = [{<<>>, <<>>}, {<<"f">>, <<"Zg==">>},
               {<<"fo">>, <<"Zm8=">>}, {<<"foo">>, <<"Zm9v">>},
               {<<"foob">>, <<"Zm9vYg==">>},
               {<<"fooba">>, <<"Zm9vYmE=">>},
               {<<"foobar">>, <<"Zm9vYmFy">>},
               {<<251,255>>, <<"+/8=">>}],
    ok = vectors(Vectors),
    <<"f">> = base64:decode(<<" Z g = =\r\n\t">>),
    <<"f">> = base64:decode(" Z g = =\r\n\t"),
    <<"Zg==">> = apply(base64, encode, [<<"f">>]),
    <<"f">> = apply(base64, decode, [<<"Zg==">>]),
    Encode = fun base64:encode/1,
    Decode = fun base64:decode/1,
    <<"Zg==">> = Encode("f"),
    <<"f">> = Decode("Zg=="),
    function_clause = failure(encode, undefined),
    function_clause = failure(decode, undefined),
    function_clause = failure(encode, <<1:1>>),
    function_clause = failure(decode, <<1:1>>),
    badarg = failure(encode, [-1]),
    badarith = failure(encode, [undefined]),
    badarith = failure(encode, [[1]]),
    function_clause = failure(encode, [65|66]),
    missing_padding = failure(decode, <<"Zg">>),
    missing_padding = failure(decode, "Zg"),
    badarg = failure(decode, <<"-w==">>),
    badarg = failure(decode, [-1]),
    badarith = failure(decode, [[65]]),
    function_clause = failure(decode, [65|66]),
    ok.

vectors([]) -> ok;
vectors([{Raw, Encoded}|Rest]) ->
    Encoded = base64:encode(Raw),
    Encoded = base64:encode(binary_to_list(Raw)),
    Raw = base64:decode(Encoded),
    Raw = base64:decode(binary_to_list(Encoded)),
    Encoded = base64:encode(Raw, #{}),
    Raw = base64:decode(Encoded, #{}),
    vectors(Rest).

failure(Function, Value) ->
    try apply(base64, Function, [Value]) of
        _ -> unexpected_success
    catch error:Reason -> Reason
    end.
