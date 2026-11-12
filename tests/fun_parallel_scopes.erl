%% Tests for parallel scope semantics around fun bodies.
%% Exercises the interaction between variable binding in tuples/lists,
%% fun body isolation, andalso/orelse short-circuit captures, and
%% the "first bind then match" rule across parallel constructs.
-module(fun_parallel_scopes).
-export([test/0]).

test() ->
    ok = tuple_capture(),
    ok = list_capture(),
    ok = fun_body_isolation(),
    ok = andalso_fun_capture(),
    ok = orelse_fun_capture(),
    ok = multiple_funs_shared_capture(),
    ok = nested_parallel_constructs(),
    ok = first_bind_then_match(),
    ok.

%% --- 1. Variable binding in tuple elements with fun capture ---
%% X is bound before the tuple; the fun captures it; the second
%% tuple element must match X (already-bound variable semantics).
tuple_capture() ->
    X = id(42),
    {F, X} = {fun() -> X end, id(42)},
    42 = F(),

    %% Fun with parameter, capturing an outer variable
    Y = id(99),
    {G, Y} = {fun(Z) -> Z + Y end, id(99)},
    109 = G(10),

    %% Tuple with multiple captures
    A = id(5),
    B = id(7),
    {H, A, B} = {fun() -> A * B end, id(5), id(7)},
    35 = H(),
    ok.

%% --- 2. List construction with fun captures ---
list_capture() ->
    A = id(10),
    B = id(20),
    [F, G] = [fun() -> A + B end, fun() -> A * B end],
    30 = F(),
    200 = G(),

    %% Three-element list: two funs plus a matched value
    C = id(3),
    [H, C, I] = [fun(X) -> X + C end, id(3), fun(X) -> X * C end],
    13 = H(10),
    30 = I(10),
    ok.

%% --- 3. Fun body variable isolation ---
%% Variables bound INSIDE a fun body must NOT leak to the outer scope.
fun_body_isolation() ->
    V = id(100),
    F = fun() ->
            Inner = 999,
            Inner + V
        end,
    1099 = F(),
    %% Inner was only bound inside the fun; we can bind it independently here
    Inner = id(55),
    55 = Inner,

    %% A fun that binds a variable with the same name as an outer one
    %% does not affect the outer binding
    X = id(hello),
    G = fun() ->
            X2 = world,
            X2
        end,
    hello = X,
    world = G(),

    %% Sequential: fun body binding does not leak even when called
    Outer = id(1),
    H = fun() ->
            Local = id(2),
            Local
        end,
    2 = H(),
    1 = Outer,
    ok.

%% --- 4. Fun in andalso RHS captures variables bound before andalso ---
andalso_fun_capture() ->
    N = id(5),
    Result = id(true) andalso begin
        F = fun() -> N * 2 end,
        F()
    end,
    10 = Result,

    %% When LHS is false, RHS never executes
    R2 = id(false) andalso begin
        _G = fun() -> error(should_not_run) end,
        error(should_not_run)
    end,
    false = R2,

    %% Fun with parameter, capturing outer variable
    Base = id(100),
    R3 = id(true) andalso begin
        H = fun(X) -> X + Base end,
        H(50)
    end,
    150 = R3,

    %% Nested andalso with fun capture
    M = id(3),
    R4 = id(true) andalso (id(true) andalso begin
        I = fun() -> M + 1 end,
        I()
    end),
    4 = R4,
    ok.

%% --- 5. Fun in orelse RHS captures outer variables ---
orelse_fun_capture() ->
    M = id(7),
    Result = id(false) orelse begin
        F = fun() -> M + 3 end,
        F()
    end,
    10 = Result,

    %% When LHS is true, RHS is skipped
    R2 = id(true) orelse begin
        _G = fun() -> error(should_not_run) end,
        error(should_not_run)
    end,
    true = R2,

    %% orelse chain with fun capture
    K = id(50),
    R3 = id(false) orelse begin
        H = fun(X) -> X + K end,
        H(25)
    end,
    75 = R3,
    ok.

%% --- 6. Multiple funs in a tuple sharing captured variables ---
multiple_funs_shared_capture() ->
    A = id(10),
    B = id(20),
    {F1, F2, F3} = {fun() -> A + B end,
                     fun() -> A * B end,
                     fun(X) -> X + A + B end},
    30 = F1(),
    200 = F2(),
    35 = F3(5),

    %% A fun capturing another fun (closure captures closure)
    Inc = fun(X) -> X + 1 end,
    Apply = fun(Val) -> Inc(Val) end,
    6 = Apply(5),

    %% Three funs, each capturing different subsets
    C = id(100),
    D = id(200),
    {G1, G2, G3} = {fun() -> C end, fun() -> D end, fun() -> C + D end},
    100 = G1(),
    200 = G2(),
    300 = G3(),
    ok.

%% --- 7. Nested parallel constructs ---
%% Tuple inside list, fun returning list of tuples, fun returning fun.
nested_parallel_constructs() ->
    Outer = id(1000),

    %% Fun that returns a list of tuples, capturing Outer
    F = fun(X) ->
            [{X, Outer}, {X + 1, Outer + 1}]
        end,
    [{5, 1000}, {6, 1001}] = F(5),

    %% Tuple of funs inside a begin block, capturing from multiple levels
    A = id(3),
    {G1, G2} = begin
                    F1 = fun(Y) -> Y + A + Outer end,
                    F2 = fun(Y) -> Y * A end,
                    {F1, F2}
                end,
    1008 = G1(5),
    15 = G2(5),

    %% Fun returning fun: inner fun captures from two scope levels
    Level1 = id(10),
    MakeAdder = fun(Base) ->
        fun(X) -> X + Base + Level1 end
    end,
    Adder = MakeAdder(100),
    115 = Adder(5),

    %% List of tuples where each tuple contains a fun
    B = id(2),
    [{H1, B}, {H2, B}] = [{fun(X) -> X + B end, id(2)},
                           {fun(X) -> X * B end, id(2)}],
    7 = H1(5),
    10 = H2(5),
    ok.

%% --- 8. First bind then match rule with funs ---
%% When a variable appears multiple times, the first occurrence
%% binds and subsequent occurrences must match.
first_bind_then_match() ->
    %% V bound by first element, second must match
    V = id(42),
    {V, V} = {id(42), id(42)},
    42 = V,

    %% Fun captures a variable that participates in matching
    W = id(77),
    {W, F} = {id(77), fun() -> W end},
    77 = F(),

    %% Three positions: match, fun capture, match again
    P = id(88),
    {P, G, P} = {id(88), fun() -> P + 1 end, id(88)},
    89 = G(),

    %% Same in list context
    Q = id(33),
    [Q, H, Q] = [id(33), fun() -> Q * 2 end, id(33)],
    66 = H(),

    %% Nested tuple with matching and fun capture
    R = id(55),
    {R, {R, I}} = {id(55), {id(55), fun() -> R end}},
    55 = I(),

    %% Match rule with fun that also takes a parameter
    S = id(10),
    {S, J, S} = {id(10), fun(X) -> X + S end, id(10)},
    15 = J(5),
    ok.

id(I) -> I.
