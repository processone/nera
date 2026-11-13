%% EXPECT: try_operation_short_circuit_ok
%% Inside a try body, an operation or throw that raises goes to the handler
%% at once, as on BEAM: no later send, put, receive or call runs, and a
%% later operation cannot replace the reason. Guards inside the body still
%% just fail. Outside try, a failed send or spawn or a throw stops the
%% process at once, and a map update evaluates every value before it fails.
%% Run this same fixture on native Nera and BEAM.
-module(try_operation_short_circuit).
-export([test/0]).

test() ->
    lists:foreach(fun check/1, protected()),
    lists:foreach(fun guard/1, guards()),
    lists:foreach(fun unprotected/1, unprotected()),
    io:format("try_operation_short_circuit_ok~n").

%% {Name, Expected handler result, Messages after it, Body}. A message
%% `queued` is in the mailbox before each body runs.
protected() ->
    [{add, {error, badarith}, [queued],
      fun() -> try _ = 1 + id(a), self() ! sent, ok catch C:E -> {C, E} end end},
     {subtract_bound, {error, badarith}, [queued],
      fun() -> try X = id(a) - 1, self() ! {sent, X}, ok catch C:E -> {C, E} end end},
     {multiply_then_put, {error, badarith}, [queued],
      fun() -> try _ = id(a) * 2, put(k, ran), ok catch C:E -> {C, E} end end},
     {int_div, {error, badarith}, [queued],
      fun() -> try _ = 1 div id(0), self() ! sent, ok catch C:E -> {C, E} end end},
     {int_rem, {error, badarith}, [queued],
      fun() -> try _ = 1 rem id(0), self() ! sent, ok catch C:E -> {C, E} end end},
     {bit_and, {error, badarith}, [queued],
      fun() -> try _ = id(a) band 1, self() ! sent, ok catch C:E -> {C, E} end end},
     {bit_or, {error, badarith}, [queued],
      fun() -> try _ = id(1.0) bor 1, self() ! sent, ok catch C:E -> {C, E} end end},
     {bit_xor, {error, badarith}, [queued],
      fun() -> try _ = id(a) bxor 1, self() ! sent, ok catch C:E -> {C, E} end end},
     {shift_left, {error, badarith}, [queued],
      fun() -> try _ = 1 bsl id(a), self() ! sent, ok catch C:E -> {C, E} end end},
     {shift_right, {error, badarith}, [queued],
      fun() -> try _ = id(a) bsr 1, self() ! sent, ok catch C:E -> {C, E} end end},
     {bool_and, {error, badarg}, [queued],
      fun() -> try _ = id(a) and true, self() ! sent, ok catch C:E -> {C, E} end end},
     {bool_or, {error, badarg}, [queued],
      fun() -> try _ = id(a) or false, self() ! sent, ok catch C:E -> {C, E} end end},
     {bool_xor, {error, badarg}, [queued],
      fun() -> try _ = id(a) xor true, self() ! sent, ok catch C:E -> {C, E} end end},
     {negate, {error, badarith}, [queued],
      fun() -> try _ = -id(a), self() ! sent, ok catch C:E -> {C, E} end end},
     {bit_not, {error, badarith}, [queued],
      fun() -> try _ = bnot id(a), self() ! sent, ok catch C:E -> {C, E} end end},
     {bool_not, {error, badarg}, [queued],
      fun() -> try _ = not id(a), self() ! sent, ok catch C:E -> {C, E} end end},
     {andalso_right, {error, badarith}, [queued],
      fun() -> try _ = id(true) andalso (1 + id(a) > 0), self() ! sent, ok catch C:E -> {C, E} end end},
     {orelse_right, {error, badarith}, [queued],
      fun() -> try _ = id(false) orelse (1 + id(a) > 0), self() ! sent, ok catch C:E -> {C, E} end end},
     {map_put_badmap, {error, {badmap, notamap}}, [queued],
      fun() -> try _ = (id(notamap))#{a => 1}, self() ! sent, ok catch C:E -> {C, E} end end},
     {map_update_badmap, {error, {badmap, notamap}}, [queued],
      fun() -> try _ = (id(notamap))#{a := 1}, self() ! sent, ok catch C:E -> {C, E} end end},
     {map_update_badkey, {error, {badkey, a}}, [queued],
      fun() -> try _ = (id(#{}))#{a := 1}, self() ! sent, ok catch C:E -> {C, E} end end},
     %% The first failed field is the reason, not the next field's update.
     {map_first_field_fails, {error, {badkey, a}}, [queued],
      fun() -> try _ = (id(#{b => 0}))#{a := 1, b := 2}, self() ! sent, ok catch C:E -> {C, E} end end},
     %% Every value is evaluated before the update fails.
     {map_values_first, {error, {badmap, notamap}}, [queued, value_sent],
      fun() -> try _ = (id(notamap))#{a => 1, b => (self() ! value_sent)}, self() ! sent, ok
               catch C:E -> {C, E} end end},
     {send_unregistered, {error, badarg}, [queued],
      fun() -> try _ = id(no_such_registered_name) ! x, self() ! sent, ok catch C:E -> {C, E} end end},
     {send_not_a_pid, {error, badarg}, [queued],
      fun() -> try _ = id(1) ! x, self() ! sent, ok catch C:E -> {C, E} end end},
     {send_tuple_unregistered, {error, badarg}, [queued],
      fun() -> try _ = id(no_such_registered_name) ! {x, id(y)}, self() ! sent, ok
               catch C:E -> {C, E} end end},
     %% Native Nera also prints a "function not found" line here.
     {spawn_bad_arguments, {error, badarg}, [queued],
      fun() -> try _ = spawn(lists, reverse, id(notalist)), self() ! sent, ok catch C:E -> {C, E} end end},
     {comprehension, {error, badarith}, [queued],
      fun() -> try _ = [1 + X || X <- id([a])], self() ! sent, ok catch C:E -> {C, E} end end},
     {comprehension_filter, {error, badarith}, [queued],
      fun() -> try _ = [X || X <- id([1]), X + id(a) > 0], self() ! sent, ok catch C:E -> {C, E} end end},
     %% A receive without after must not take the queued message.
     {then_receive, {error, badarith}, [queued],
      fun() -> try X = 1 + id(a), receive M -> {got, M, X} end catch C:E -> {C, E} end end},
     {then_local_call, {error, badarith}, [queued],
      fun() -> try _ = 1 + id(a), side_effect() catch C:E -> {C, E} end end},
     {reason_kept, {error, badarith}, [queued],
      fun() -> try X = 1 + id(a), element(X, {1}) catch C:E -> {C, E} end end},
     {tuple_element, {error, badarith}, [queued],
      fun() -> try {1 + id(a), self() ! sent} catch C:E -> {C, E} end end},
     {call_argument, {error, badarith}, [queued],
      fun() -> try pair(1 + id(a), self() ! sent) catch C:E -> {C, E} end end},
     {left_operand, {error, badarith}, [queued],
      fun() -> try (1 + id(a)) + (self() ! 1) catch C:E -> {C, E} end end},
     {case_body, {error, badarith}, [queued],
      fun() -> try case id(x) of x -> _ = 1 + id(a), self() ! sent, ok end catch C:E -> {C, E} end end},
     {try_of, {error, badarith}, [queued],
      fun() -> try _ = 1 + id(a), self() ! sent of _ -> no_exception catch C:E -> {C, E} end end},
     {throw_mid_body, {throw, stop}, [queued],
      fun() -> try _ = case id(bad) of bad -> throw(stop); W -> W end, self() ! sent, ok
               catch C:E -> {C, E} end end},
     {throw_reason_kept, {throw, stop}, [queued],
      fun() -> try V = case id(bad) of bad -> throw(stop); W -> W end, V + 1 catch C:E -> {C, E} end end},
     {catch_throw, stop, [queued],
      fun() -> catch begin _ = case id(bad) of bad -> throw(stop); W -> W end, self() ! sent, ok end end},
     {catch_expression, 'EXIT', [queued],
      fun() -> element(1, catch begin _ = 1 + id(a), self() ! sent, ok end) end},
     {inner_try_rethrows, {outer, error, badarith}, [queued],
      fun() -> try try _ = 1 + id(a), self() ! sent, ok catch throw:_ -> inner end
               catch C:E -> {outer, C, E} end end},
     {inner_of_clause, {outer, error, badarith}, [queued],
      fun() -> try try id(1) of X -> _ = X + id(a), self() ! sent, ok catch C:E -> {inner, C, E} end
               catch C2:E2 -> {outer, C2, E2} end end},
     {inner_catch_clause, {outer, error, badarith}, [queued],
      fun() -> try try throw(x) catch throw:x -> _ = 1 + id(a), self() ! sent end
               catch C:E -> {outer, C, E} end end},
     %% The after body still runs once, after the handler.
     {catch_and_after, {error, badarith}, [queued, cleaned],
      fun() -> try _ = 1 + id(a), self() ! sent, ok catch C:E -> {C, E} after self() ! cleaned end end},
     {after_only, {outer, error, badarith}, [queued, cleaned],
      fun() -> try try _ = 1 + id(a), self() ! sent, ok after self() ! cleaned end
               catch C:E -> {outer, C, E} end end},
     {no_error, ok, [queued, {sent, 3, [{a, 2}, {b, 3}]}],
      fun() -> try X = 1 + id(2), M = (id(#{a => 1}))#{a := 2, b => 3},
                   self() ! {sent, X, lists:sort(maps:to_list(M))}, ok
               catch C:E -> {C, E} end end}].

check({Name, Expected, ExpectedMessages, Body}) ->
    flush(),
    erase(k),
    self() ! queued,
    {Name, Expected, ExpectedMessages, undefined} = {Name, Body(), flush(), get(k)}.

%% A guard that raises inside a try body fails the guard; the body goes on.
guards() ->
    [{guard_arith, fun() -> try case id(a) of X when X + 1 > 0 -> wrong; _ -> right end
                            catch C:E -> {C, E} end end},
     {guard_not, fun() -> try case id(a) of X when not X -> wrong; _ -> right end
                          catch C:E -> {C, E} end end},
     {guard_and, fun() -> try case id(a) of X when X and true -> wrong; _ -> right end
                          catch C:E -> {C, E} end end},
     {guard_alternative, fun() -> try case id(a) of X when X * 2 > 0; X =:= a -> right; _ -> wrong end
                                  catch C:E -> {C, E} end end},
     {function_guard, fun() -> try clause_guard(id(a)) catch C:E -> {C, E} end end}].

guard({Name, Body}) ->
    {Name, right} = {Name, Body()}.

%% Outside try, the process stops at the failed operation. BEAM's exit
%% reason carries the stack, and an uncaught throw's is {nocatch, Reason};
%% native Nera's are not yet.
unprotected() ->
    [{send_unregistered, badarg, [],
      fun(P) -> _ = id(no_such_registered_name) ! x, P ! sent end},
     {send_tuple_unregistered, badarg, [],
      fun(P) -> _ = id(no_such_registered_name) ! {x, id(y)}, P ! sent end},
     {throw_mid_body, stop, [],
      fun(P) -> _ = case id(bad) of bad -> throw(stop); W -> W end, P ! sent end},
     {spawn_bad_arguments, badarg, [],
      fun(P) -> _ = spawn(lists, reverse, id(notalist)), P ! sent end},
     {map_values_first, {badmap, notamap}, [value_sent],
      fun(P) -> _ = (id(notamap))#{a => 1, b => (P ! value_sent)}, P ! sent end}].

unprotected({Name, Expected, ExpectedMessages, Body}) ->
    flush(),
    P = self(),
    {Pid, Ref} = spawn_monitor(fun() -> Body(P) end),
    Reason = receive {'DOWN', Ref, process, Pid, R} -> exit_reason(R) end,
    {Name, Expected, ExpectedMessages} = {Name, Reason, flush()}.

exit_reason({Reason, [_ | _]}) -> exit_reason(Reason);
exit_reason({nocatch, Reason}) -> Reason;
exit_reason(Reason) -> Reason.

clause_guard(X) when X + 1 > 0 -> wrong;
clause_guard(_) -> right.

side_effect() -> put(k, side_effect_ran), ok.

pair(A, B) -> {A, B}.

flush() ->
    receive M -> [M | flush()] after 0 -> [] end.

id(X) -> X.
