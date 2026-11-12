%% EXPECT: module_info_exports_ok
-module(module_info_exports).
-compile({parse_transform, eunit_autoexport}).
-export([test/0]).
test() ->
    Exports = ?MODULE:module_info(exports),
    true = lists:member({test, 0}, Exports),
    true = lists:member({sample_test, 0}, Exports),
    true = lists:member({sample_test_, 0}, Exports),
    false = lists:member({private_helper, 0}, Exports),
    false = lists:member({other_test, 1}, Exports),
    module_info_exports = ?MODULE:module_info(module),
    try ?MODULE:module_info(unsupported_field) of _ -> error(expected_badarg)
    catch error:badarg -> ok end,
    32 = $ ,
    ok = sample_test(),
    io:format("module_info_exports_ok~n"),
    ok.
sample_test() -> private_helper().
sample_test_() -> [fun() -> ok end].
private_helper() -> ok.
other_test(X) -> X.
