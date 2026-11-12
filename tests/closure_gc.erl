-module(closure_gc).
-export([test/0]).

%% Regression test: closures must survive GC.
%% The bug was that CallTarget::Closure VarIds were not tracked as uses
%% in the liveness analysis, so closure variables were not saved in
%% resume continuations when a callee yielded. GC then moved the
%% closure on the heap without updating the stale reference.

test() ->
    %% Test 1: closure survives GC triggered by heavy allocation
    test_closure_gc_basic(),

    %% Test 2: closure with captured complex data survives GC
    test_closure_gc_complex(),

    %% Test 3: multiple closures survive GC
    test_multi_closure_gc(),

    %% Test 4: closure used after two GC-triggering calls
    test_closure_double_gc(),

    ok.

test_closure_gc_basic() ->
    Data = [1, 2, 3, 4, 5],
    F = fun() -> length(Data) end,
    5 = F(),
    _ = make_junk(50000),
    5 = F(),
    ok.

test_closure_gc_complex() ->
    M = #{a => 1, b => 2, c => 3},
    L = [{x, 10}, {y, 20}, {z, 30}],
    F = fun() -> {maps:get(a, M), length(L)} end,
    {1, 3} = F(),
    _ = make_junk(50000),
    {1, 3} = F(),
    ok.

test_multi_closure_gc() ->
    A = [1, 2, 3],
    B = [4, 5, 6],
    F1 = fun() -> length(A) end,
    F2 = fun() -> length(B) end,
    3 = F1(),
    3 = F2(),
    _ = make_junk(50000),
    3 = F1(),
    3 = F2(),
    ok.

test_closure_double_gc() ->
    Data = [10, 20, 30],
    F = fun() -> length(Data) end,
    3 = F(),
    _ = make_junk(50000),
    3 = F(),
    _ = make_junk(50000),
    3 = F(),
    ok.

make_junk(0) -> ok;
make_junk(N) ->
    _ = {N, N+1, N+2, N+3, N+4, N+5, N+6, N+7, N+8, N+9},
    make_junk(N - 1).
