%% Ported from Erlang/OTP lib/compiler/test/fun_SUITE.erl
%% Covers: test1, overwritten_fun, otp_7202, bif_fun, external,
%%         eep37 (named funs), badarity, badfun, duplicated_fun,
%%         unused_fun, coverage, leaky_environment.
-module(fun_suite).
-export([test/0, call_me/1, dup1/0, dup2/0, helper/1]).

-record(b, {c}).

test() ->
    ok = test1(),
    ok = overwritten_fun(),
    ok = otp_7202(),
    ok = bif_fun(),
    ok = external(),
    ok = eep37(),
    ok = badarity(),
    ok = badfun(),
    ok = duplicated_fun(),
    ok = unused_fun(),
    ok = coverage(),
    ok = leaky_environment(),
    ok.

%% --- test1: various fun shapes ---
test1() ->
    %% Fresh parameter name in fun shadows outer binding.
    1 = begin _A = 3, F = fun(_A) -> 1 end, F(2) end,
    %% Non-matching literal pattern triggers function_clause.
    ok = begin
             G = fun(1 = 0) -> ok end,
             try G(2), not_reached
             catch error:function_clause -> ok
             end
         end,
    %% Self-recursive via passed-in fun argument.
    SelfFact = fun(_, 1) -> 1; (F, N) -> N * F(F, N - 1) end,
    120 = SelfFact(SelfFact, 5),
    1 = SelfFact(SelfFact, 1),
    ok.

%% --- OTP-7102: overwritten fun variable ---
overwritten_fun() ->
    {a2, a} = overwritten_fun_1(a),
    {a2, {b, c}} = overwritten_fun_1(#b{c = c}),
    one = overwritten_fun_1(#b{c = []}),
    ok.

overwritten_fun_1(A) ->
    F = fun() -> {ok, A} end,
    if A#b.c == [] ->
            one;
       true ->
            case F() of
                {ok, A2} -> {a2, A2};
                _ -> three
            end
    end.

%% --- OTP-7202: liveness across make_fun2 ---
otp_7202() ->
    otp_7202_do().

otp_7202_do() ->
    List = [a],
    Error = case otp_7202_func() of
                no_value -> true;
                {ok, V} -> V
            end,
    ok = lists:foreach(fun(_E) ->
                               case Error of
                                   true -> ok;
                                   false -> ok
                               end
                       end, List),
    ok.

otp_7202_func() -> no_value.

%% --- fun BIF/1 reference (via explicit module) ---
bif_fun() ->
    F = fun erlang:abs/1,
    5 = F(id(-5)),
    ok.

%% --- external M:F/A refs, static and dynamic ---
external() ->
    Mod = id(?MODULE),
    Func = id(call_me),
    Arity = id(1),

    %% Avoid unused var warnings for now unused dynamic pieces.
    _ = {Mod, Func, Arity},

    {ok, {a, b}} = (fun ?MODULE:call_me/1)({a, b}),
    {ok, {a, b}} = (fun fun_suite:call_me/1)({a, b}),
    apply_cm(fun ?MODULE:call_me/1),
    apply_cm(fun fun_suite:call_me/1),

    %% Dynamic application via apply/3.
    {ok, hello} = apply(Mod, Func, [hello]),
    {ok, there} = apply(?MODULE, call_me, [there]),

    %% Dynamic application via apply/2 (Fun + Args list).
    42 = apply(fun erlang:abs/1, [id(-42)]),
    6 = apply(fun lists:sum/1, [[1, 2, 3]]),
    {ok, hi} = apply(fun ?MODULE:call_me/1, [hi]),
    5 = apply(fun(X) -> X + 1 end, [4]),
    hello = apply(fun() -> hello end, []),
    7 = apply(fun(A, B) -> A + B end, [3, 4]),

    check_map_ref(fun lists:map/2),

    42 = (fun erlang:abs/1)(id(-42)),
    42 = (id(fun erlang:abs/1))(id(-42)),
    42 = apply(erlang, abs, [id(-42)]),
    6 = (fun lists:sum/1)([1, 2, 3]),
    6 = (id(fun lists:sum/1))([1, 2, 3]),

    %% Badarity when calling external fun with wrong arity.
    ok = try (fun lists:sum/1)(1, 2, 3), not_reached
         catch error:{badarity, {Ref, [1, 2, 3]}} when is_function(Ref, 1) -> ok
         end,

    ok.

apply_cm(F) ->
    {ok, {a, b}} = F({a, b}),
    ok.

check_map_ref(Map) ->
    Id = fun(I) -> I end,
    List = [x, y],
    List = Map(Id, List),
    ok.

call_me(I) -> {ok, I}.

helper(X) -> X + 1.

%% --- EEP-37: named (recursive) funs ---
eep37() ->
    ok = eep37_basic(),
    ok = eep37_dup(),
    ok.

eep37_basic() ->
    %% Single-clause named funs (name position is legal but may be unused).
    Add = fun _(N) -> N + 1 end,
    UnusedName = fun _BlackAdder(N) -> N + 42 end,
    10 = Add(9),
    50 = UnusedName(8),
    ok.

eep37_dup() ->
    dup1 = (dup1())(),
    dup2 = (dup2())(),
    ok.

dup1() -> fun _F1() -> dup1 end.
dup2() -> fun _F2() -> dup2 end.

%% --- badarity: calling fun with wrong arity ---
badarity() ->
    F = fun(X) -> X end,
    ok = try F(), not_reached
         catch error:{badarity, {F, []}} -> ok
         end,
    ok = try (fun() -> 42 end)(0), not_reached
         catch error:{badarity, {G, [0]}} when is_function(G, 0) -> ok
         end,
    ok.

%% --- badfun: applying a non-fun value ---
badfun() ->
    X = id(not_a_fun),
    expect_badfun(42, catch (id(42))()),
    expect_badfun(42.0, catch (id(42.0))(1)),
    expect_badfun(X, catch X()),
    expect_badfun(X, catch X(1)),
    Len = length(atom_to_list(X)),
    expect_badfun(Len, catch (id(Len))(1)),

    %% Multi-clause fun where the matched literal argument isn't callable.
    Literal = fun(literal = Fun) -> Fun() end,
    expect_badfun(literal, catch Literal(literal)),
    ok.

expect_badfun(Term, {'EXIT', {{badfun, Term}, _}}) -> ok;
expect_badfun(Term, {'EXIT', {badfun, Term}}) -> ok.

%% --- duplicated_fun: fun variable in pattern alias ---
duplicated_fun() ->
    %% `X = fun f/1` should match (both sides are funs), but equality
    %% with an atom head via `=` must fail.
    ok = try
             id([print_result_paths_fun = fun duplicated_fun_helper/1]),
             not_reached
         catch
             error:{badmatch, F} when is_function(F, 1) -> ok
         end,
    ok.

duplicated_fun_helper(_) -> ok.

%% --- unused_fun: constructing funs whose value is discarded ---
unused_fun() ->
    _ = fun() -> ok end,
    _ = try id(ok) of
            _ -> fun() -> ok end
        catch _ -> ok end,
    ok.

%% --- coverage: miscellaneous fun code paths ---
coverage() ->
    ok = coverage_1(),
    [2, 3, 4] = coverage_2(id([1, 2, 3])),
    {42, F} = coverage_3(id({[], x})),
    x = F(),
    ok.

coverage_1() ->
    %% Construct-and-discard a multi-clause fun via catch.
    catch fun(whatever) -> 0;
             ("abc") -> party
          end,
    ok.

coverage_2(List) ->
    lists:map(fun(E) -> E + 1 end, List).

coverage_3({[], A}) ->
    {id(42), fun() -> A end}.

%% --- leaky_environment: closure arity is its declared arity ---
leaky_environment() ->
    G = fun(X, Y) -> X + Y end,
    F = fun(A) -> G(A, 0) end,
    ok = try F(1, flurb), not_reached
         catch
             error:{badarity, {F, [1, flurb]}} when is_function(F, 1) -> ok
         end,
    ok.

id(I) -> I.
