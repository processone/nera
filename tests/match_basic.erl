%% Adapted from OTP compiler/test/match_SUITE.erl (Apache 2.0)
%% Tests basic pattern matching in function heads and case expressions.
-module(match_basic).
-export([test/0]).

test() ->
    %% Literal matching
    ok = literal_match(42),
    ok = literal_match(hello),
    ok = literal_match(3.14),

    %% Variable binding
    {ok, 42} = var_bind(42),
    {ok, hello} = var_bind(hello),

    %% Tuple matching
    {1, 2, 3} = tuple_match({1, 2, 3}),
    first = tuple_element({first, second, third}),
    second = tuple_element2({first, second, third}),

    %% List matching
    1 = list_head([1, 2, 3]),
    [2, 3] = list_tail([1, 2, 3]),
    empty = list_head([]),

    %% Nested pattern matching
    {ok, [1, 2]} = nested({ok, [1, 2, 3]}),

    %% Wildcard
    ok = wildcard(anything),
    ok = wildcard({complex, [stuff]}),

    %% Atom matching in case
    yes = is_bool(true),
    yes = is_bool(false),
    no = is_bool(maybe),

    %% Integer pattern matching
    zero = int_class(0),
    one = int_class(1),
    other = int_class(42),

    %% Multiple clause matching
    a = multi(1),
    b = multi(2),
    c = multi(3),
    other = multi(99),

    io:format("ok~n"),
    ok.

literal_match(42) -> ok;
literal_match(hello) -> ok;
literal_match(3.14) -> ok;
literal_match(_) -> error.

var_bind(X) -> {ok, X}.

tuple_match({A, B, C}) -> {A, B, C}.

tuple_element({First, _, _}) -> First.
tuple_element2({_, Second, _}) -> Second.

list_head([]) -> empty;
list_head([H | _]) -> H.

list_tail([_ | T]) -> T.

nested({ok, [A, B | _]}) -> {ok, [A, B]}.

wildcard(_) -> ok.

is_bool(X) ->
    case X of
        true -> yes;
        false -> yes;
        _ -> no
    end.

int_class(0) -> zero;
int_class(1) -> one;
int_class(_) -> other.

multi(1) -> a;
multi(2) -> b;
multi(3) -> c;
multi(_) -> other.
