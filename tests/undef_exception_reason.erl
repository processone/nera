-module(undef_exception_reason).
-export([start/0]).

start() ->
    M = no_such_module,
    F = no_such_function,

    %% Test 1: Dynamic call - try/catch
    try M:F() of
        _ -> error(should_have_raised)
    catch
        error:undef -> ok
    end,

    %% Test 2: Verify reason is exactly 'undef'
    Reason = try M:F()
             catch error:R -> R
             end,
    undef = Reason,

    %% Test 3: erlang:apply/3 with unknown function
    Reason2 = try erlang:apply(M, F, [])
              catch error:R2 -> R2
              end,
    undef = Reason2,

    %% Test 4: Known module, unknown function
    Reason3 = try erlang:apply(erlang, definitely_not_a_function, [])
              catch error:R3 -> R3
              end,
    undef = Reason3,

    %% Test 5: Wrong arity via dynamic apply
    F2 = length,
    Reason4 = try erlang:apply(erlang, F2, [a, b, c])
              catch error:R4 -> R4
              end,
    undef = Reason4,

    %% Test 6: Remote fun ref
    Reason5 = try
                  Fun = fun M:F/0,
                  Fun()
              catch error:R5 -> R5
              end,
    undef = Reason5,

    %% Test 7: catch expression (not try/catch)
    CatchResult = catch M:F(),
    {'EXIT', {undef, _Stacktrace}} = CatchResult,

    ok.
