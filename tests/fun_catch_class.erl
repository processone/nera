%% EXPECT: fun_catch_class_ok
%% Bound catch classes are reads, even when no guard or body reads the name.
%% Run the same assertions natively and on BEAM.
-module(fun_catch_class).
-export([test/0]).

test() ->
    right = requested(),
    lists:foreach(fun({Class, Raised, Expected}) ->
        F = capture(Class, Raised),
        Expected = F(),
        Outer = nested(Class, Raised),
        Inner = Outer(),
        Expected = Inner(),
        Recursive = named(Class, Raised),
        Expected = Recursive(2)
    end, [
        {error, throw, other}, {throw, throw, matched},
        {error, error, matched}, {throw, error, other},
        {exit, exit, matched}, {error, exit, other},
        {not_a_class, throw, other}
    ]),
    Comprehension = comprehension(id(error)),
    [other, matched, other] = Comprehension(),
    {error, matched, other} = shadowed_head(),
    Fresh = fun() -> try throw(x) catch Class:_ -> Class end end,
    throw = Fresh(),
    io:format("fun_catch_class_ok~n"),
    ok.

requested() ->
    C = id(error),
    F = fun() -> try throw(x) catch C:_ -> wrong; _:_ -> right end end,
    F().

capture(C, Raised) ->
    fun() -> try raise(Raised) catch C:_ -> matched; _:_ -> other end end.

nested(C, Raised) ->
    fun() ->
        fun() -> try raise(Raised) catch C:_ -> matched; _:_ -> other end end
    end.

named(C, Raised) ->
    fun Loop(0) -> try raise(Raised) catch C:_ -> matched; _:_ -> other end;
        Loop(N) -> Loop(N - 1)
    end.

comprehension(C) ->
    fun() ->
        [try raise(Raised) catch C:_ -> matched; _:_ -> other end
            || Raised <- id([throw, error, exit])]
    end.

shadowed_head() ->
    C = id(error),
    F = fun(C) -> try throw(x) catch C:_ -> matched; _:_ -> other end end,
    {C, F(throw), F(error)}.

raise(throw) -> throw(x);
raise(error) -> erlang:error(x);
raise(exit) -> exit(x).

id(X) -> X.
