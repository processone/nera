%% EXPECT: unary_plus_ok
%% Unary plus returns numbers unchanged and raises error:badarith for every
%% other type. Run the same fixture natively and on BEAM.
-module(unary_plus).
-export([test/0]).

test() ->
    %% Variables and calls must retain the numeric type, value and float bits.
    Numbers = [0, 1, -1, 576460752303423487, -576460752303423488,
               9223372036854775808, -9223372036854775809, 1 bsl 1100,
               0.0, -0.0, 1.5, -2.5, 5.0e-324, 1.7976931348623157e308],
    lists:foreach(fun(X) ->
        true = X =:= +X,
        true = X =:= +id(X),
        case is_float(X) of
            true -> Bits = <<X/float>>, Bits = <<(+id(X))/float>>;
            false -> ok
        end
    end, Numbers),
    %% Literal operands and unary precedence, including adjacent signs.
    1 = +1,
    1.5 = +1.5,
    9223372036854775808 = +9223372036854775808,
    <<1:1, 0:63>> = <<(+-0.0)/float>>,
    7 = +id(3) * 2 + 1,
    10 = +(id(3) + 2) * 2,
    3 = +(+id(3)),
    -3 = -+id(3),
    -3 = +-id(3),
    %% Non-numbers must not be accepted as a no-op, including an unused result.
    Bad = [a, true, [], [1], [1 | a], {}, {1}, #{}, <<>>, <<1:1>>,
           self(), make_ref(), fun() -> ok end],
    lists:foreach(fun(X) ->
        {error, badarith} = protect(fun() -> +id(X) end),
        {error, badarith} = protect(fun() -> +id(X), ignored end),
        no = guarded(X)
    end, Bad),
    {error, badarith} = protect(fun() -> +a end),
    %% A raising operation in a try stops the body before its next effect.
    {error, badarith} = try
        +id(a),
        self() ! ran_after_plus,
        wrong
    catch C:R -> {C, R} end,
    empty = receive Message -> {leaked, Message} after 0 -> empty end,
    %% Guard errors only reject their alternative; they do not escape.
    yes = guarded(id(3)),
    yes = guarded(id(1.5)),
    yes = guard_alternative(id(a)),
    [1, 2.0] = [X || X <- id([a, 1, [], 2.0]), +X > 0],
    %% Evaluate the operand once, and retain an operand's own exception.
    put(plus_count, 0),
    42 = +counted(42),
    1 = get(plus_count),
    {error, operand_error} = protect(fun() -> +erlang:error(operand_error) end),
    io:format("unary_plus_ok~n"),
    ok.

guarded(X) when +X > 0 -> yes;
guarded(_) -> no.

guard_alternative(X) when +X > 0; X =:= a -> yes;
guard_alternative(_) -> no.

counted(X) -> put(plus_count, get(plus_count) + 1), X.

protect(F) ->
    try F() of Value -> {value, Value}
    catch Class:Reason -> {Class, Reason} end.

id(X) -> X.
