-module(binary_float_bits).
-export([test/0]).

test() ->
    lists:foreach(fun(Value) -> bad(fun() -> put(Value,64,0,big) end) end,
                  [invalid, [], {}, <<>>, (1 bsl 2048)]),
    lists:foreach(fun(N) -> bad(fun() -> put(1.0,N,0,big) end) end,
                  [-1,0,8,24,48,65,1.5,invalid]),
    lists:foreach(fun(N) -> limit(fun() -> put(1.0,N,0,big) end) end,
                  [1152921504606846975,1152921504606846976]),
    caught = try <<(id(invalid)):64/float>>, error(continued)
             catch error:badarg -> caught end,
    caught = try <<1.0:(id(24))/float>>, error(continued)
             catch error:badarg -> caught end,
    {Pid, Ref} = spawn_monitor(fun() -> put(invalid,64,0,big), error(continued) end),
    receive {'DOWN',Ref,process,Pid,badarg} -> ok;
            {'DOWN',Ref,process,Pid,{badarg,_}} -> ok
    after 2000 -> error(missing_error) end,
    <<63,128,0,0>> = scaled(id(16)),
    bad(fun() -> scaled(12) end),
    limit(fun() -> wrapped_unit(id(576460752303423489)) end),
    lists:foreach(fun({Value,H,S,D}) ->
        lists:foreach(fun(P) ->
            lists:foreach(fun({Bits,Raw}) ->
                lists:foreach(fun(Endian) ->
                    Bytes = bytes(Raw,Bits,Endian),
                    Expected = <<1:P,Bytes/binary,3:2>>,
                    Expected = put(Value,Bits,P,Endian),
                    {Value,3} = get(Expected,Bits,P,Endian),
                    Large = <<0:1024,Expected/bitstring>>,
                    <<_:1024,Tail/bitstring>> = Large,
                    {Value,3} = get(Tail,Bits,P,Endian)
                end, [big,little,native])
            end, [{16,H},{32,S},{64,D}])
        end, lists:seq(0,7))
    end, [{0.0,0,0,0},{-0.0,16#8000,16#80000000,16#8000000000000000},
          {1.0,16#3C00,16#3F800000,16#3FF0000000000000},
          {-2.5,16#C100,16#C0200000,16#C004000000000000}]),
    <<60,0>> = half(id(1.00048828125)),
    <<60,1>> = half(id(1.0004882812500002)),
    <<0,1>> = half(id(0.000000059604644775390625)),
    <<124,0>> = half(id(65520.0)),
    <<63,240,0,0,0,0,0,0>> = plain(id(1)),
    <<67,176,0,0,0,0,0,0>> = plain(id(1152921504606846976)),
    lists:foreach(fun(P) ->
        lists:foreach(fun({Bits,Raw}) ->
            Input = <<1:P,Raw:Bits>>,
            fail = match_float(Input,Bits,P),
            fail = match_wildcard(Input,Bits,P)
        end, [{16,16#7C00},{16,16#7E00},{32,16#7F800000},
              {32,16#7FC00000},{64,16#7FF0000000000000},{64,16#7FF8000000000000}]),
        fail = match_float(<<1:P,0:15>>,16,P),
        fail = match_float(<<1:P,0:31>>,32,P),
        fail = match_float(<<1:P,0:63>>,64,P)
    end, lists:seq(0,7)),
    fail = match_float(<<0:24>>,24,0),
    %% Independent arithmetic oracle for every binary16 bit pattern.
    lists:foreach(fun(Raw) ->
        case Raw band 16#7C00 of
            16#7C00 -> fail = match_float(<<Raw:16>>,16,0);
            _ ->
                ExpectedHalf = reference_half(Raw),
                ExpectedHalf = match_float(<<Raw:16>>,16,0),
                <<Raw:16>> = half(ExpectedHalf)
        end
    end, lists:seq(0,65535)),
    ok.

put(X,N,P,big) -> <<1:P,X:N/float,3:2>>;
put(X,N,P,little) -> <<1:P,X:N/float-little,3:2>>;
put(X,N,P,native) -> <<1:P,X:N/float-native,3:2>>.
get(Bin,N,P,big) -> <<_:P,X:N/float,T:2>> = Bin, {X,T};
get(Bin,N,P,little) -> <<_:P,X:N/float-little,T:2>> = Bin, {X,T};
get(Bin,N,P,native) -> <<_:P,X:N/float-native,T:2>> = Bin, {X,T}.
bytes(Raw,N,big) -> <<Raw:N>>;
bytes(Raw,N,little) -> <<Raw:N/little>>;
bytes(Raw,N,native) -> <<Raw:N/native>>.
match_float(Bin,N,P) -> case Bin of <<_:P,X:N/float>> -> X; _ -> fail end.
match_wildcard(Bin,N,P) -> case Bin of <<_:P,_:N/float>> -> matched; _ -> fail end.
scaled(N) -> <<1.0:N/float-unit:2>>.
wrapped_unit(N) -> <<1.0:N/float-unit:32>>.
half(X) -> <<X:16/float>>.
plain(X) -> <<X:64/float>>.
id(X) -> X.
bad(F) -> try F(), error(expected_badarg) catch error:badarg -> ok end.

limit(F) -> try F(), error(expected_limit) catch error:system_limit -> ok end.

reference_half(Raw) ->
    E = (Raw bsr 10) band 31,
    F = Raw band 1023,
    {M,Power} = case E of 0 -> {F,-24}; _ -> {1024+F,E-25} end,
    Magnitude = case Power >= 0 of
        true -> (M * (1 bsl Power)) / 1;
        false -> M / (1 bsl (-Power))
    end,
    case Raw band 16#8000 of 0 -> Magnitude; _ -> -Magnitude end.
