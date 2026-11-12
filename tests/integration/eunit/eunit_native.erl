%% EXPECT: eunit_native_ok
-module(eunit_native).
-include_lib("eunit/include/eunit.hrl").
-export([test/0]).

test() ->
    ok = eunit:test(?MODULE, [verbose]),
    ok = eunit:test([fun() -> non_ok_result end,
                    {"nested", [fun() -> ok end, fun() -> 42 end]}]),
    error = eunit:test(fun() -> error(expected_failure) end),
    error = eunit:test({generator, fun() -> error(generator_failed) end}),
    error = eunit:test(fun() -> exit(killed_test) end),
    error = eunit:test({timeout, 0.01, fun() -> receive never -> ok end end}),
    ok = eunit:test(fun() -> undefined = get(eunit_isolation), put(eunit_isolation, value) end),
    undefined = get(eunit_isolation),
    error = eunit:test({unsupported_descriptor, fun() -> ok end}),
    io:format("eunit_native_ok~n"),
    ok.

basic_test() -> ?assertEqual(2, 1 + 1).
values_test_() -> [{integer_to_list(N), fun() -> ?assertEqual(N, N) end} || N <- [1, 2, 3]].
