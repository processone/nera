%% EXPECT: lc_bad_generator_ok
%% Regression: a list generator whose source is not a proper list raises
%% error:{bad_generator, Term} as on BEAM, in list and binary comprehensions.
%% Term is the source itself, or the improper tail the generator reaches.
%% Before the fix, Nera raised error:badarg for [Y || Y <- id(a)],
%% [Y || Y <- id([1 | a])] and << Y || Y <- id([<<1>> | a]) >>.
%% Elements before an improper tail are still produced, filtered and
%% generated from; the error is raised when the generator reaches the tail.
%% Run this same fixture on native Nera and BEAM.
-module(lc_bad_generator).
-export([test/0]).

test() ->
    lists:foreach(fun({Name, Expected, Fun}) ->
        case result(Fun) of
            Expected -> ok;
            Actual -> erlang:error({wrong_result, Name, Expected, Actual})
        end
    end, cases()),
    traces(),
    uncaught(),
    io:format("lc_bad_generator_ok~n"),
    ok.

cases() ->
    reported() ++ other_sources() ++ improper_tails() ++ nested() ++ valid().

%% The reported cases.
reported() ->
    [{atom_source, bad(a), fun() -> [Y || Y <- id(a)] end},
     {improper_list, bad(a), fun() -> [Y || Y <- id([1 | a])] end},
     {binary_comprehension, bad(a), fun() -> << Y || Y <- id([<<1>> | a]) >> end}].

%% Sources of other types are reported unchanged.
other_sources() ->
    Fun = fun id/1,
    [{integer, bad(1), fun() -> [Y || Y <- id(1)] end},
     {big_integer, bad(1 bsl 70), fun() -> [Y || Y <- id(1 bsl 70)] end},
     {float, bad(1.5), fun() -> [Y || Y <- id(1.5)] end},
     {tuple, bad({1, 2}), fun() -> [Y || Y <- id({1, 2})] end},
     {map, bad(#{a => 1}), fun() -> [Y || Y <- id(#{a => 1})] end},
     {binary, bad(<<1, 2>>), fun() -> [Y || Y <- id(<<1, 2>>)] end},
     {function, bad(Fun), fun() -> [Y || Y <- id(Fun)] end},
     {binary_comprehension_atom, bad(a), fun() -> << Y || Y <- id(a) >> end},
     {binary_generator, bad(a), fun() -> [X || <<X>> <= id(a)] end}].

%% The improper tail is reported, whatever the elements before it did.
improper_tails() ->
    [{binary_tail, bad(<<3>>), fun() -> [Y || Y <- id([1, 2 | <<3>>])] end},
     {tuple_tail, bad({t}), fun() -> [Y || Y <- id([1 | {t}])] end},
     {filtered_in, bad(a), fun() -> [Y || Y <- id([1, 2 | a]), Y > 1] end},
     {filtered_out, bad(a), fun() -> [Y || Y <- id([1, 2 | a]), Y > 5] end},
     {pattern_skip, bad(a), fun() -> [Y || {Y} <- id([{1}, x | a])] end},
     {binary_body, bad(a), fun() -> << <<Y>> || Y <- id([1 | a]) >> end}].

%% Each generator is checked where it runs, inner or outer.
nested() ->
    [{inner_atom, bad(b), fun() -> [{X, Y} || X <- id([1, 2]), Y <- id(b)] end},
     {inner_improper, bad(b), fun() -> [{X, Y} || X <- id([1, 2]), Y <- id([y | b])] end},
     {outer_improper, bad(c), fun() -> [{X, Y} || X <- id([1 | c]), Y <- id([y])] end},
     {inner_from_outer, bad(2), fun() -> [Y || X <- id([[1], 2]), Y <- X] end},
     {inner_from_binding, bad(1), fun() -> [Y || X <- id([1]), Z <- [X], Y <- Z] end},
     {leading_filter, bad(a), fun() -> [Y || id(true), Y <- id(a)] end},
     {binary_nested, bad(d), fun() -> << <<X, Y>> || X <- id([1, 2]), Y <- id([3 | d]) >> end},
     {after_binary_generator, bad(e), fun() -> << <<X, Y>> || <<X>> <= id(<<1, 2>>), Y <- id(e) >> end}].

%% Proper lists, and generators that never run, still succeed.
valid() ->
    [{empty, [], fun() -> [Y || Y <- id([])] end},
     {proper, [2, 4, 6], fun() -> [Y * 2 || Y <- id([1, 2, 3])] end},
     {string, [$a, $b], fun() -> [Y || Y <- id("ab")] end},
     {binary_proper, <<1, 2, 3>>, fun() -> << <<Y>> || Y <- id([1, 2, 3]) >> end},
     {inner_never_runs, [], fun() -> [{X, Y} || X <- id([]), Y <- id(a)] end},
     {leading_false, [], fun() -> [Y || id(false), Y <- id(a)] end}].

%% Bodies, filters and inner generators run for each element before the tail.
traces() ->
    {{error, {bad_generator, a}}, [{body, 1}, {body, 2}]} =
        traced(fun() -> [trace(Y) || Y <- id([1, 2 | a])] end),
    {{error, {bad_generator, a}}, [{filter, 1}, {body, 1}, {filter, 2}, {body, 2}]} =
        traced(fun() -> [trace(Y) || Y <- id([1, 2 | a]), filter(Y)] end),
    {{error, {bad_generator, a}}, [{body, <<1>>}, {body, <<2>>}]} =
        traced(fun() -> << <<(trace(Y))/binary>> || Y <- id([<<1>>, <<2>> | a]) >> end),
    {{error, {bad_generator, bad}}, [{gen, [y1]}, {body, {1, y1}}, {gen, [y2 | bad]}, {body, {2, y2}}]} =
        traced(fun() -> [trace({X, Y}) || X <- id([1, 2]), Y <- gen(X)] end),
    ok.

%% Outside any try, the bad_generator error ends the process with the error
%% and its stack trace as the exit reason.
uncaught() ->
    {Pid, Ref} = spawn_monitor(fun() -> [Y || Y <- id([1 | a])] end),
    receive
        {'DOWN', Ref, process, Pid, {{bad_generator, a}, Stack}} when is_list(Stack) -> ok;
        {'DOWN', Ref, process, Pid, Other} -> erlang:error({wrong_exit, Other})
    after 5000 -> erlang:error(no_down)
    end.

bad(Term) -> {error, {bad_generator, Term}}.

gen(1) -> trace_gen([y1]);
gen(2) -> trace_gen([y2 | bad]).

trace_gen(L) -> put(trace, [{gen, L} | get(trace)]), L.

traced(Fun) ->
    put(trace, []),
    Result = result(Fun),
    {Result, lists:reverse(erase(trace))}.

result(Fun) ->
    try Fun() catch Class:Reason -> {Class, Reason} end.

trace(X) -> put(trace, [{body, X} | get(trace)]), X.

filter(X) -> put(trace, [{filter, X} | get(trace)]), true.

id(X) -> X.
