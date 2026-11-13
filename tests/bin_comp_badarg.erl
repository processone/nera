%% EXPECT: bin_comp_badarg_ok
%% Regression: a binary comprehension whose body is not a bitstring raises
%% error:badarg at the iteration that yields it, as on BEAM. Before the fix,
%% Nera joined the bodies like an iolist, so << (id(1)) || _ <- [a] >>
%% returned <<1>> and << (id([1,2])) || _ <- [a] >> returned <<1,2>>.
%% BEAM appends each body as <<Acc/bitstring, Body/bitstring>>: the failing
%% iteration's body runs, then no later filter or body does.
%% Run this same fixture on native Nera and BEAM.
-module(bin_comp_badarg).
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
    io:format("bin_comp_badarg_ok~n"),
    ok.

cases() ->
    reported() ++ other_terms() ++ generator_values() ++ body_exceptions() ++ valid().

%% The reported cases.
reported() ->
    [{integer, {error, badarg}, fun() -> << (id(1)) || _ <- [a] >> end},
     {byte_list, {error, badarg}, fun() -> << (id([1, 2])) || _ <- [a] >> end}].

%% Other terms that are not bitstrings, iodata included.
other_terms() ->
    Bad = {error, badarg},
    [{binary_list, Bad, fun() -> << (id([<<1>>])) || _ <- [a] >> end},
     {nil, Bad, fun() -> << (id([])) || _ <- [a] >> end},
     {atom, Bad, fun() -> << (id(a)) || _ <- [a] >> end},
     {float, Bad, fun() -> << (id(1.0)) || _ <- [a] >> end},
     {tuple, Bad, fun() -> << (id({<<1>>})) || _ <- [a] >> end},
     {big_integer, Bad, fun() -> << (id(1 bsl 70)) || _ <- [a] >> end}].

%% Generator values used directly as the body.
generator_values() ->
    Bad = {error, badarg},
    [{list_values, Bad, fun() -> << X || X <- id([1, 2]) >> end},
     {binary_values, Bad, fun() -> << X || <<X>> <= id(<<1, 2>>) >> end},
     {late_bad_value, Bad, fun() -> << X || X <- id([<<1>>, <<2>>, 3]) >> end},
     {inner_generator, Bad, fun() -> << X || X <- id([<<1>>, 2]), _ <- id([a]) >> end},
     {filtered_in, Bad, fun() -> << X || X <- id([<<1>>, 2]), X =/= <<1>> >> end}].

%% An exception raised by the body itself is kept, not replaced.
body_exceptions() ->
    [{body_raises, {error, boom}, fun() -> << (error(boom)) || _ <- [a] >> end},
     {body_throws, {throw, t}, fun() -> << (throw(t)) || _ <- [a] >> end}].

%% Comprehensions that never append a bad body still succeed.
valid() ->
    [{no_iterations, <<>>, fun() -> << X || X <- id([]) >> end},
     {filtered_out, <<2>>, fun() -> << X || X <- id([1, <<2>>, a]), is_binary(X) >> end},
     {bitstrings, <<34>>, fun() -> << X || X <- id([<<1:3>>, <<2:5>>]) >> end},
     {binary_body, <<1, 2>>, fun() -> << <<X>> || X <- id([1, 2]) >> end},
     {nested_body, <<1, 2, 1, 2>>, fun() -> << << <<X>> || X <- id([1, 2]) >> || _ <- id([a, b]) >> end}].

%% The failing iteration's body runs; later filters and bodies do not.
traces() ->
    {{error, badarg}, [{body, <<1>>}, {body, 2}]} =
        traced(fun() -> << (trace(X)) || X <- id([<<1>>, 2, <<3>>]) >> end),
    {{error, badarg}, [{filter, <<1>>}, {body, <<1>>}, {filter, 2}, {body, 2}]} =
        traced(fun() -> << (trace(X)) || X <- id([<<1>>, 2, <<3>>]), filter(X) >> end),
    {{error, badarg},
     [{filter, {<<1>>, y1}}, {body, <<1>>}, {filter, {<<1>>, y2}}, {body, <<1>>},
      {filter, {2, y1}}, {body, 2}]} =
        traced(fun() -> << (trace(X)) || X <- id([<<1>>, 2]), Y <- id([y1, y2]), filter({X, Y}) >> end),
    ok.

%% Outside any try, the badarg ends the process.
uncaught() ->
    {Pid, Ref} = spawn_monitor(fun() -> << (id(1)) || _ <- [a] >> end),
    receive
        {'DOWN', Ref, process, Pid, badarg} -> ok;
        {'DOWN', Ref, process, Pid, {badarg, _}} -> ok
    after 5000 -> erlang:error(no_down)
    end.

traced(Fun) ->
    put(trace, []),
    Result = result(Fun),
    {Result, lists:reverse(erase(trace))}.

result(Fun) ->
    try Fun() catch Class:Reason -> {Class, Reason} end.

trace(X) -> put(trace, [{body, X} | get(trace)]), X.

filter(X) -> put(trace, [{filter, X} | get(trace)]), true.

id(X) -> X.
