%% EXPECT: andalso_orelse_badarg_ok
%% `andalso` and `orelse` whose left side is not a boolean raise
%% error:{badarg, Left} before the right side runs, as on BEAM: inside try
%% the handler gets it and no later code runs; outside try the process
%% stops there. The right side's value is never checked. A guard or map
%% pattern key with such a left side just fails, as does any map pattern key
%% that raises. Run this same fixture on native Nera and BEAM.
-module(andalso_orelse_badarg).
-export([test/0]).

test() ->
    lists:foreach(fun check/1, protected()),
    lists:foreach(fun value/1, values()),
    lists:foreach(fun guard/1, guards()),
    lists:foreach(fun guard/1, map_keys()),
    lists:foreach(fun unprotected/1, unprotected()),
    io:format("andalso_orelse_badarg_ok~n").

%% {Name, Expected handler result, Messages after it, Body}. A message
%% `queued` is in the mailbox before each body runs.
protected() ->
    [{andalso_integer, {error, {badarg, 5}}, [queued],
      fun() -> try _ = id(5) andalso true, self() ! sent, ok catch C:E -> {C, E} end end},
     {orelse_integer, {error, {badarg, 5}}, [queued],
      fun() -> try _ = id(5) orelse true, self() ! sent, ok catch C:E -> {C, E} end end},
     {andalso_atom, {error, {badarg, foo}}, [queued],
      fun() -> try id(foo) andalso true catch C:E -> {C, E} end end},
     {orelse_tuple, {error, {badarg, {a, b}}}, [queued],
      fun() -> try id({a, b}) orelse false catch C:E -> {C, E} end end},
     {andalso_right_skipped, {error, {badarg, 5}}, [queued],
      fun() -> try id(5) andalso (self() ! right_ran) =:= x catch C:E -> {C, E} end end},
     {orelse_right_skipped, {error, {badarg, 5}}, [queued],
      fun() -> try id(5) orelse (self() ! right_ran) =:= x catch C:E -> {C, E} end end},
     {nested_left, {error, {badarg, 5}}, [queued],
      fun() -> try (id(5) andalso true) orelse true catch C:E -> {C, E} end end},
     {nested_right, {error, {badarg, 7}}, [queued],
      fun() -> try id(true) andalso (id(7) orelse true) catch C:E -> {C, E} end end},
     {case_scrutinee, {error, {badarg, 5}}, [queued],
      fun() -> try case id(5) andalso true of true -> yes; _ -> other end catch C:E -> {C, E} end end},
     {call_argument, {error, {badarg, 5}}, [queued],
      fun() -> try id(id(5) orelse false) catch C:E -> {C, E} end end},
     {tail_position, {error, {badarg, 5}}, [queued],
      fun() -> try tail_andalso(5) catch C:E -> {C, E} end end},
     {fun_body, {error, {badarg, 5}}, [queued],
      fun() -> F = fun(X) -> X andalso true end, try F(id(5)) catch C:E -> {C, E} end end},
     {try_of, {error, {badarg, 5}}, [queued],
      fun() -> try id(5) orelse true of V -> {no_exception, V} catch C:E -> {C, E} end end},
     {match_true, {error, {badarg, 5}}, [queued],
      fun() -> try true = id(5) andalso true catch C:E -> {C, E} end end},
     {comprehension_filter, {error, {badarg, 5}}, [queued],
      fun() -> try [Y || Y <- id([true, 5]), id(Y) andalso true] catch C:E -> {C, E} end end},
     {catch_expression, {'EXIT', {badarg, 5}}, [queued],
      fun() -> case catch (id(5) andalso true) of
                   {'EXIT', {Reason, _Stack}} -> {'EXIT', Reason};
                   Other -> Other
               end end},
     %% The after body still runs once.
     {after_only, {outer, error, {badarg, 5}}, [queued, cleaned],
      fun() -> try try _ = id(5) andalso true, self() ! sent after self() ! cleaned end
               catch C:E -> {outer, C, E} end end}].

check({Name, Expected, ExpectedMessages, Body}) ->
    flush(),
    self() ! queued,
    {Name, Expected, ExpectedMessages} = {Name, Body(), flush()}.

%% A boolean left side works as before, and the right side's value is
%% returned unchecked.
values() ->
    [{andalso_true_any, 5, fun() -> id(true) andalso id(5) end},
     {orelse_false_any, 5, fun() -> id(false) orelse id(5) end},
     {andalso_false, false, fun() -> id(false) andalso id(5) end},
     {orelse_true, true, fun() -> id(true) orelse id(5) end},
     {comparison_left, 5, fun() -> (id(1) < 2) andalso id(5) end},
     {nested_right_value, 7, fun() -> id(true) andalso (id(false) orelse id(7)) end},
     {tail_position, true, fun() -> tail_andalso(true) end}].

value({Name, Expected, Body}) ->
    {Name, Expected} = {Name, Body()}.

%% A guard with a non-boolean left side fails; nothing is raised.
guards() ->
    [{clause, no, fun() -> guard_andalso(5) end},
     {clause_true, yes, fun() -> guard_andalso(true) end},
     {clause_orelse, no, fun() -> guard_orelse(5) end},
     {under_not, no, fun() -> guard_not(5) end},
     {compared, no, fun() -> guard_compared(5) end},
     {next_alternative, second, fun() -> guard_alternative(5) end},
     {type_test, big, fun() -> guard_type_test(5) end},
     {if_guard, other, fun() -> X = id(5), if X andalso true -> yes; true -> other end end},
     {comprehension_guard, [true], fun() -> [Y || Y <- id([5, true, false]), Y andalso true] end},
     {inside_try, other,
      fun() -> try case id(5) of X when X andalso true -> yes; _ -> other end
               catch C:E -> {C, E} end end}].

guard({Name, Expected, Body}) ->
    {Name, Expected} = {Name, Body()}.

%% A map pattern key that raises makes the pattern not match.
map_keys() ->
    [{case_clause, nomatch, fun() -> map_key(5, id(#{5 => a, true => b})) end},
     {case_clause_true, b, fun() -> map_key(true, id(#{5 => a, true => b})) end},
     {orelse_false, c, fun() -> map_key_orelse(false, id(#{false => c})) end},
     {match_expression, {error, {badmatch, #{5 => a}}},
      fun() -> K = id(5), try #{(K orelse false) := V} = id(#{5 => a}), V catch C:E -> {C, E} end end},
     {fun_head, nomatch,
      fun() -> K = id(5), F = fun(#{(K andalso true) := V}) -> V; (_) -> nomatch end, F(id(#{5 => a})) end},
     {receive_pattern, {none, [#{5 => a}]},
      fun() -> K = id(5), self() ! #{5 => a},
               R = receive #{(K orelse false) := V} -> {got, V} after 0 -> none end,
               {R, flush()} end},
     {arithmetic, nomatch,
      fun() -> K = id(a), case id(#{5 => a}) of #{(K + 1) := V} -> V; _ -> nomatch end end}].

%% Outside try, the process stops at the failed operator.
unprotected() ->
    [{andalso_integer, {badarg, 5}, [], fun(P) -> _ = id(5) andalso true, P ! sent end},
     {orelse_atom, {badarg, bad}, [], fun(P) -> _ = id(bad) orelse true, P ! sent end},
     {tail_position, {badarg, 5}, [before], fun(P) -> P ! before, tail_andalso(5) end},
     {boolean, normal, [sent], fun(P) -> _ = id(true) andalso true, P ! sent end}].

unprotected({Name, Expected, ExpectedMessages, Body}) ->
    flush(),
    P = self(),
    {Pid, Ref} = spawn_monitor(fun() -> Body(P) end),
    Reason = receive {'DOWN', Ref, process, Pid, R} -> exit_reason(R) end,
    {Name, Expected, ExpectedMessages} = {Name, Reason, flush()}.

%% This fixture owns the reason, not the exit envelope's stack.
exit_reason({Reason, [_ | _]}) -> Reason;
exit_reason(Reason) -> Reason.

tail_andalso(X) -> X andalso true.

guard_andalso(X) when X andalso true -> yes;
guard_andalso(_) -> no.

guard_orelse(X) when X orelse false -> yes;
guard_orelse(_) -> no.

guard_not(X) when not (X andalso true) -> yes;
guard_not(_) -> no.

guard_compared(X) when (X andalso true) =:= 5 -> yes;
guard_compared(_) -> no.

guard_alternative(X) when X andalso true; X =:= 5 -> second;
guard_alternative(_) -> no.

guard_type_test(X) when is_integer(X) andalso X > 3 -> big;
guard_type_test(_) -> no.

map_key(K, M) ->
    case M of #{(K andalso true) := V} -> V; _ -> nomatch end.

map_key_orelse(K, M) ->
    case M of #{(K orelse false) := V} -> V; _ -> nomatch end.

flush() ->
    receive M -> [M | flush()] after 0 -> [] end.

id(X) -> X.
