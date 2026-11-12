%% Transport only: Thoas itself is compiled unchanged from the pinned checkout.
-module(native_bridge).
-export([start/0]).
start() ->
    {ok, Data} = file:read_file("request.etf"),
    {Module, Function, Args} = binary_to_term(Data),
    Result = try dispatch(Module, Function, Args) of
                 Value -> {return, Value}
             catch Class:Reason -> {exception, Class, Reason}
             end,
    ok = file:write_file("reply.etf", term_to_binary(Result)).

%% Explicit calls isolate library behaviour from the runtime apply/3 path.
dispatch(thoas, decode, [A0]) -> thoas:decode(A0);
dispatch(thoas, decode, [A0, A1]) -> thoas:decode(A0, A1);
dispatch(thoas, encode, [A0]) -> thoas:encode(A0);
dispatch(thoas, encode, [A0, A1]) -> thoas:encode(A0, A1);
dispatch(thoas, encode_to_iodata, [A0]) -> thoas:encode_to_iodata(A0);
dispatch(thoas, encode_to_iodata, [A0, A1]) -> thoas:encode_to_iodata(A0, A1);
dispatch(thoas_encode, true, []) -> thoas_encode:true();
dispatch(thoas_encode, false, []) -> thoas_encode:false();
dispatch(thoas_encode, null, []) -> thoas_encode:null();
dispatch(thoas_encode, boolean, [A0]) -> thoas_encode:boolean(A0);
dispatch(thoas_encode, integer, [A0]) -> thoas_encode:integer(A0);
dispatch(thoas_encode, float, [A0]) -> thoas_encode:float(A0);
dispatch(thoas_encode, string, [A0]) -> thoas_encode:string(A0);
dispatch(thoas_encode, non_recursive_array, [A0]) -> thoas_encode:non_recursive_array(A0);
dispatch(thoas_encode, non_recursive_object, [A0]) -> thoas_encode:non_recursive_object(A0);
dispatch(thoas_encode, encode, [A0, A1]) -> thoas_encode:encode(A0, A1).
