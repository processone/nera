%% EXPECT: lists_member_badarg_ok
%% Regression: OTP 29 lists:member/2 raises badarg for a non-list argument
%% or an improper tail reached before a match, but returns an earlier match.
%% Run this same fixture on native Nera and BEAM.
-module(lists_member_badarg).
-export([test/0]).

test() ->
    Tails = id([not_a_list, 42, 1.5, 1 bsl 70, {}, #{key => value},
                <<>>, <<1:1>>, self(), make_ref(), fun id/1]),
    %% Long enough for BEAM to trap and resume before reaching the tail.
    Long = id(lists:seq(1, 100000)),
    lists:foreach(fun(Mode) ->
        Member = calls(Mode),
        lists:foreach(fun(Tail) ->
            badarg = failure(fun() -> Member(needle, Tail) end),
            badarg = failure(fun() -> Member(needle, [other | Tail]) end),
            %% The tail itself is not a list element, even if it equals Query.
            badarg = failure(fun() -> Member(Tail, [other, another | Tail]) end),
            %% Do not validate the remainder after a successful match.
            true = Member(needle, [needle | Tail]),
            true = Member(needle, [other, needle | Tail]),
            %% An improper list is a valid element; only the outer spine matters.
            true = Member([other | Tail], [[other | Tail] | Tail])
        end, Tails),
        %% A numerically equal head is not an exact match before a bad tail.
        badarg = failure(fun() -> Member(1.0, id([1 | invalid_tail])) end),
        true = Member(1.0, id([1, 1.0 | invalid_tail])),
        badarg = failure(fun() -> Member(needle, Long ++ invalid_tail) end),
        true = Member(needle, Long ++ [needle | invalid_tail]),
        io:format("~p: non-lists and reached tails raise badarg; earlier matches return true~n", [Mode])
    end, [direct, apply_call, dynamic, remote_fun]),
    io:format("lists_member_badarg_ok~n"),
    ok.

calls(direct) -> fun(Q, L) -> lists:member(Q, L) end;
calls(apply_call) -> fun(Q, L) -> apply(id(lists), id(member), [Q, L]) end;
calls(dynamic) ->
    M = id(lists), F = id(member),
    fun(Q, L) -> M:F(Q, L) end;
calls(remote_fun) -> fun lists:member/2.

failure(F) ->
    try F() of Value -> {unexpected_return, Value}
    catch error:Reason -> Reason
    end.

id(X) -> X.
