%% EXPECT: lists_member_exact_ok
%% NERA-89: lists:member/2 uses =:=, including OTP 27+ signed-zero distinctions.
%% Run this same regression on native Nera and BEAM.
-module(lists_member_exact).
-export([test/0]).

test() ->
    Big = id(1) bsl 100,
    Ref = make_ref(),
    F = fun id/1,
    Cases = [
        {1.0, 1, false},
        {-0.0, 0.0, false},
        {Big, float(Big), false},
        {{child, [1]}, {child, [1.0]}, false},
        {[child | 1], [child | 1.0], false},
        {#{state => 1}, #{state => 1.0}, false},
        {#{1 => active}, #{1.0 => active}, false},
        {{child, [0.0]}, {child, [-0.0]}, false},
        {#{state => 0.0}, #{state => -0.0}, false},
        {#{0.0 => active}, #{-0.0 => active}, false},
        {1, id(1), true},
        {1.0, id(1.0), true},
        {0.0, id(0.0), true},
        {-0.0, id(-0.0), true},
        {Big, id(1) bsl 100, true},
        {{child, [1]}, {id(child), [id(1)]}, true},
        {[child | 1], [id(child) | id(1)], true},
        {#{a => 1, b => active}, #{b => id(active), a => id(1)}, true},
        {<<"child">>, list_to_binary(id("child")), true},
        {<<1:3>>, <<(id(1)):3>>, true},
        {<<1:3>>, <<1:4>>, false},
        {self(), self(), true},
        {Ref, Ref, true},
        {Ref, make_ref(), false},
        {F, F, true},
        %% NERA-98 I1: flat-string comparisons use the cons fast path.
        {"GET", binary_to_list(id(<<"GET">>)), true},
        {"GET", "PUT", false},
        {"GET", "GET /", false},
        {"GET /", "GET /status", false}
    ],
    lists:foreach(fun(Mode) ->
        Member = calls(Mode),
        lists:foreach(fun({Query, Candidate, Same}) ->
            check_pair(Member, Query, Candidate, Same),
            check_pair(Member, Candidate, Query, Same)
        end, Cases)
    end, [direct, apply_call, dynamic, remote_fun]),
    %% Unlike member/2, the key-search family intentionally uses numeric ==.
    {key, 1} = lists:keyfind(id(1.0), 2, id([{key, 1}])),
    true = lists:keymember(id(-0.0), 2, id([{key, 0.0}])),
    {value, {key, 1}} = lists:keysearch(id(1.0), 2, id([{key, 1}])),
    io:format("lists_member_exact_ok~n"),
    ok.

check_pair(Member, Query, Candidate, Same) ->
    Same = Query =:= Candidate,
    false = Member(Query, []),
    Same = Member(Query, [Candidate]),
    Same = Member(Query, [missing, Candidate, missing]),
    %% A near-equal candidate must not prevent finding a later exact match.
    true = Member(Query, [Candidate, Query]),
    ok.

calls(direct) -> fun(Q, L) -> lists:member(Q, L) end;
calls(apply_call) -> fun(Q, L) -> apply(id(lists), id(member), [Q, L]) end;
calls(dynamic) ->
    M = id(lists), F = id(member),
    fun(Q, L) -> M:F(Q, L) end;
calls(remote_fun) -> fun lists:member/2.

id(X) -> X.
