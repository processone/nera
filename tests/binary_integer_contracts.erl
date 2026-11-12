%% EXPECT: integer contracts passed
-module(binary_integer_contracts).
-export([test/0]).

test() ->
    lists:foreach(fun(X) ->
        bad(fun() -> put(X,8,0,big) end),
        bad(fun() -> put(X,0,0,big) end)
    end, [invalid,[],{},<<>>,1.5]),
    lists:foreach(fun(X) ->
        bad(fun() -> binary(X,1) end),
        bad(fun() -> binary(X,0) end),
        bad(fun() -> all_binary(X) end)
    end, [invalid,[],{},1,1.5]),
    bad(fun() -> binary(<<1>>,2) end),
    bad(fun() -> all_binary(<<1:1>>) end),
    <<>> = binary(<<1>>,0),
    <<1>> = binary(<<1,2>>,1),
    <<1:1>> = all_bits(<<1:1>>),
    caught = try <<(id(invalid)):8>>, error(continued) catch error:badarg -> caught end,
    caught = try <<(id(invalid)):1/binary>>, error(continued) catch error:badarg -> caught end,
    {Pid,Ref} = spawn_monitor(fun() -> put(invalid,8,0,big), error(continued) end),
    receive {'DOWN',Ref,process,Pid,badarg} -> ok;
            {'DOWN',Ref,process,Pid,{badarg,_}} -> ok
    after 2000 -> error(missing_error) end,
    lists:foreach(fun({X,N}) ->
        lists:foreach(fun(P) ->
            lists:foreach(fun(E) ->
                Expected = reference(X,N,P,E),
                Expected = put(X,N,P,E),
                Mask = (1 bsl N)-1,
                Unsigned = X band Mask,
                Unsigned = get(Expected,N,P,E,unsigned),
                Signed = case N > 0 andalso (Unsigned band (1 bsl (N-1))) /= 0 of
                    true -> Unsigned-(1 bsl N); false -> Unsigned end,
                Signed = get(Expected,N,P,E,signed)
            end, [big,little,native])
        end, lists:seq(0,7))
    end, [{0,0},{0,1},{1,1},{-1,1},{16#ABC,12},{-37,13},
          {16#FEDCBA,24},{1 bsl 60,61},{(1 bsl 64)-1,64},
          {(1 bsl 64)+3,65},{-1,65},{1,80},{-123456789,80},
          {(1 bsl 127)+123,128},{(1 bsl 1023)+17,1024},
          {-((1 bsl 1023)+17),1025}]),
    lists:foreach(fun(N) ->
        fail = match(<<0:64>>,N),
        fail = wildcard(<<0:64>>,N)
    end, [-1,invalid,1.5,1152921504606846976]),
    fail = match(<<0:7>>,8),
    fail = match_unit(<<0:64>>,576460752303423489),
    fail = enormous_constant(<<0:64>>),
    fail = enormous_sum(<<0:64>>),
    fail = followed_by_byte(<<0:8>>,8),
    0 = followed_by_byte(<<0:8>>,0),
    fail = utf_after_dynamic(<<194,162>>,0),
    {162,7} = utf_after_dynamic(<<194,162,7>>,0),
    fail = two_dynamic_fields(<<1,2>>,8,0),
    {2,0,3} = two_dynamic_fields(<<1,2,3>>,8,0),
    io:format("integer contracts passed~n"),
    ok.

put(X,N,P,big) -> <<5:P,X:N,3:2>>;
put(X,N,P,little) -> <<5:P,X:N/little,3:2>>;
put(X,N,P,native) -> <<5:P,X:N/native,3:2>>.
get(B,N,P,big,unsigned) -> <<_:P,X:N,T:2>> = B, 3=T, X;
get(B,N,P,little,unsigned) -> <<_:P,X:N/little,T:2>> = B, 3=T, X;
get(B,N,P,native,unsigned) -> <<_:P,X:N/native,T:2>> = B, 3=T, X;
get(B,N,P,big,signed) -> <<_:P,X:N/signed,T:2>> = B, 3=T, X;
get(B,N,P,little,signed) -> <<_:P,X:N/signed-little,T:2>> = B, 3=T, X;
get(B,N,P,native,signed) -> <<_:P,X:N/signed-native,T:2>> = B, 3=T, X.
binary(X,N) -> <<X:N/binary>>.
all_binary(X) -> <<X/binary>>.
all_bits(X) -> <<X/bitstring>>.
match(B,N) -> case B of <<X:N,_/bitstring>> -> X; _ -> fail end.
wildcard(B,N) -> case B of <<_:N,_/bitstring>> -> matched; _ -> fail end.
match_unit(B,N) -> case B of <<X:N/unit:32,_/bitstring>> -> X; _ -> fail end.
enormous_constant(B) -> case B of <<_:1152921504606846975/unit:256>> -> matched; _ -> fail end.
enormous_sum(B) -> case B of <<_:1152921504606846975,_:1152921504606846975,_:1152921504606846975,_:1152921504606846975,_:1152921504606846975,_:1152921504606846975,_:1152921504606846975,_:1152921504606846975,_:1152921504606846975>> -> matched; _ -> fail end.
followed_by_byte(B,N) -> case B of <<_:N,X:8>> -> X; _ -> fail end.
utf_after_dynamic(B,N) -> case B of <<_:N,X/utf8,T:8>> -> {X,T}; _ -> fail end.
two_dynamic_fields(B,P,N) -> case B of <<_:P,X:8,Y:N,Z:8>> -> {X,Y,Z}; _ -> fail end.
id(X) -> X.
bad(F) -> try F(), error(expected_badarg) catch error:badarg -> ok end.

%% Independent bit-by-bit wire oracle, including a partial final little-endian byte.
reference(X,N,P,native) ->
    case <<1:16/native>> of <<1,0>> -> reference(X,N,P,little); _ -> reference(X,N,P,big) end;
reference(X,N,P,E) -> <<5:P,(wire(X,N,0,E,<<>>))/bitstring,3:2>>.
wire(_,N,N,_,Acc) -> Acc;
wire(X,N,I,E,Acc) ->
    K = case E of
        big -> N-1-I;
        little -> Start=(I div 8)*8, Start+erlang:min(8,N-Start)-1-(I rem 8)
    end,
    Bit=(X bsr K) band 1,
    wire(X,N,I+1,E,<<Acc/bitstring,Bit:1>>).
