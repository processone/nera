-module(function_macros).
-export([test/0]).

test() ->
    %% ?FUNCTION_NAME returns the current function name as an atom
    test = ?FUNCTION_NAME,
    0 = ?FUNCTION_ARITY,

    %% Works in different functions with different arities
    {foo, 1} = foo(hello),
    {bar, 2} = bar(1, 2),

    %% Works in expressions
    true = is_atom(?FUNCTION_NAME),
    true = is_integer(?FUNCTION_ARITY),

    ok.

foo(_X) ->
    {?FUNCTION_NAME, ?FUNCTION_ARITY}.

bar(_X, _Y) ->
    {?FUNCTION_NAME, ?FUNCTION_ARITY}.
