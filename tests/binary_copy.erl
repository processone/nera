%% EXPECT: binary_copy_ok
%% Intent: binary:copy/2 must raise OTP-compatible errors instead of aborting,
%% and preserve ordinary results and caller state when copying suspends.
-module(binary_copy).
-export([test/0]).

test() ->
    lists:foreach(fun({Bin, Count, Expected}) ->
        Expected = binary:copy(id(Bin), id(Count)),
        Expected = apply(binary, copy, [Bin, Count]),
        Copy = fun binary:copy/2,
        Expected = Copy(Bin, Count)
    end, [{<<"abc">>, 3, <<"abcabcabc">>},
          {<<>>, (1 bsl 64)-1, <<>>},
          {<<"source">>, 0, <<>>}]),
    lists:foreach(fun({Bin, Count, Reason}) ->
        expect_error(fun() -> binary:copy(Bin, Count) end, Reason),
        expect_error(fun() -> apply(binary, copy, [Bin, Count]) end, Reason),
        Copy = fun binary:copy/2,
        expect_error(fun() -> Copy(Bin, Count) end, Reason)
    end, [{<<"123456789">>, (1 bsl 60)-1, system_limit},
          {<<"0123456789abcdef0">>, (1 bsl 60)-1, system_limit},
          {<<"x">>, 1 bsl 61, system_limit},
          {<<>>, 1 bsl 64, badarg},
          {<<"x">>, 1 bsl 64, badarg},
          {<<>>, -1, badarg}, {<<"x">>, -(1 bsl 80), badarg},
          {<<>>, not_integer, badarg}, {<<"x">>, 1.0, badarg},
          {not_binary, 0, badarg}, {<<1:1>>, 0, badarg}]),
    %% The original crash must become an ordinary monitored child error, with
    %% the parent reaching the completion marker rather than aborting or hanging.
    {Pid, Ref} = spawn_monitor(fun() ->
        binary:copy(id(<<"123456789">>), id((1 bsl 60)-1))
    end),
    receive
        {'DOWN', Ref, process, Pid, {system_limit, Stack}} when is_list(Stack) -> ok
    after 5000 -> error(missing_system_limit_down)
    end,
    %% Real outputs straddle native chunks and repetition boundaries. The live
    %% parent binary and try handler must survive all suspensions.
    lists:foreach(fun(Count) ->
        Expected = list_to_binary(lists:duplicate(Count, "abc")),
        Wrapped = <<42,Expected/binary,43>>,
        Subject = binary:part(Wrapped, 1, byte_size(Expected)),
        Triple = <<Expected/binary,Expected/binary,Expected/binary>>,
        Triple = try binary:copy(Subject, 3)
                 catch C:R -> error({unexpected_copy_error,C,R}) end,
        Triple = apply(binary, copy, [Subject, 3]),
        42 = binary:at(Wrapped, 0),
        43 = binary:at(Wrapped, byte_size(Wrapped)-1)
    end, [21845,21846,43691]),
    io:format("binary_copy_ok~n"),
    ok.

expect_error(Fun, Expected) ->
    Parent = self(),
    Expected = try
        Fun(),
        Parent ! copy_returned_after_error,
        no_error
    catch error:Reason -> Reason end,
    receive copy_returned_after_error -> error(copy_did_not_raise)
    after 0 -> ok end.

id(Value) -> Value.
