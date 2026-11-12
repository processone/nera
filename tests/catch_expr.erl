-module(catch_expr).
-export([test/0]).

%% Test the unary `catch Expr` operator.

test() ->
    %% Normal value passes through
    42 = catch 42,
    %% Arithmetic inside catch
    3 = catch (1 + 2),
    %% Throw returns the thrown value directly
    hello = catch throw(hello),
    %% Exit wraps in {'EXIT', Reason}
    {'EXIT', reason} = catch exit(reason),
    %% Error wraps in {'EXIT', {Reason, Stacktrace}}
    {'EXIT', {badarg, _}} = catch error(badarg),
    %% catch in case expression
    ok = test_case_catch(),
    %% Nested catch
    ok = test_nested_catch(),
    ok.

test_case_catch() ->
    case catch throw(hello) of
        hello -> ok;
        _ -> error
    end.

test_nested_catch() ->
    Result = catch exit(bye),
    case Result of
        {'EXIT', bye} -> ok;
        _ -> error
    end.
