%% EXPECT: eunit_runtime_metadata_ok
-module(eunit_runtime_metadata).
-export([test/0, identity/1]).
identity(X) -> X.
test() ->
    Remote = fun ?MODULE:identity/1,
    {module, ?MODULE} = erlang:fun_info(Remote, module),
    {name, identity} = erlang:fun_info(Remote, name),
    {arity, 1} = erlang:fun_info(Remote, arity),
    {?MODULE, identity, 1} = erlang:fun_info_mfa(Remote),
    Anonymous = fun(X) -> X end,
    {module, ?MODULE} = erlang:fun_info(Anonymous, module),
    {name, Name} = erlang:fun_info(Anonymous, name), true = is_atom(Name),
    {arity, 1} = erlang:fun_info(Anonymous, arity),
    {?MODULE, Name, 1} = erlang:fun_info_mfa(Anonymous),
    ok = try erlang:fun_info(not_fun, arity), error(unexpected) catch error:badarg -> ok end,
    ok = try erlang:fun_info(Remote, invalid), error(unexpected) catch error:badarg -> ok end,
    {Total, Delta} = statistics(wall_clock),
    true = is_integer(Total), true = Total >= 0,
    true = Delta >= 0, true = Delta =< Total,
    {Later, _} = statistics(wall_clock), true = Later >= Total,
    ok = try statistics(invalid), error(unexpected) catch error:badarg -> ok end,
    "value:42" = lists:flatten(io_lib:fwrite("value:~w", [42])),
    "cow_http:12: sample_test" = lists:flatten(io_lib:fwrite("~ts:~ts ~ts", [cow_http,"12:",sample_test])),
    "value:42" = lists:flatten(io_lib:fwrite(["value:", ["~w"]], [42])),
    io:fwrite("eunit_runtime_metadata_ok~n", []).
