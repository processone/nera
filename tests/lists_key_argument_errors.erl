%% EXPECT: lists_key_argument_errors_ok
%% Regression: the lists key functions reject the arguments OTP 29 rejects.
%% keyreplace/4 and keystore/4 take only a tuple replacement (function_clause);
%% keyfind/3, keymember/3 and keysearch/3 take only an immediate integer
%% position of at least 1, and a list that is proper up to the first match
%% (badarg). Run this same fixture on native Nera and BEAM.
-module(lists_key_argument_errors).
-export([test/0]).

test() ->
    Rows = id([{a,1},{b,2}]),
    %% Largest immediate integer on 64-bit BEAM; one more is a bignum.
    MaxSmall = id((1 bsl 59) - 1),
    %% Long enough that BEAM's search traps and resumes before the tail.
    Long = id([{n, I} || I <- lists:seq(1, 100000)]),
    lists:foreach(fun(Mode) ->
        {Replace, Store} = updates(Mode),
        %% A non-tuple replacement fails at the entry, whether or not a key matches.
        lists:foreach(fun(New) ->
            lists:foreach(fun({Key, List}) ->
                function_clause = failure(fun() -> Replace(Key, 1, List, New) end),
                function_clause = failure(fun() -> Store(Key, 1, List, New) end)
            end, [{a, []}, {a, Rows}, {z, Rows}])
        end, id([not_a_tuple, [x], 7, 1.5, #{}, <<"t">>])),
        %% The empty tuple is still a tuple.
        [{},{b,2}] = Replace(a, 1, Rows, id({})),
        [{a,1},{b,2},{}] = Store(z, 1, Rows, id({})),

        {Find, Member, Search} = searches(Mode),
        lists:foreach(fun(N) ->
            lists:foreach(fun(List) ->
                badarg = failure(fun() -> Find(a, N, List) end),
                badarg = failure(fun() -> Member(a, N, List) end),
                badarg = failure(fun() -> Search(a, N, List) end)
            end, [[], Rows])
        end, id([0, -1, 1.0, position, MaxSmall + 1, 1 bsl 70])),
        %% The largest valid position is accepted and simply finds nothing.
        lists:foreach(fun(List) ->
            false = Find(a, MaxSmall, List),
            false = Member(a, MaxSmall, List),
            false = Search(a, MaxSmall, List)
        end, [[], Rows]),

        %% A list argument that is not a list, or an improper tail reached
        %% before any match, is badarg for atom, integer, float and tuple keys.
        lists:foreach(fun({Key, N, List}) ->
            badarg = failure(fun() -> Find(Key, N, List) end),
            badarg = failure(fun() -> Member(Key, N, List) end),
            badarg = failure(fun() -> Search(Key, N, List) end)
        end, id([{a, 1, foo}, {a, 1, 42}, {a, 1, 1.5}, {a, 1, {a, 1}},
                 {a, 1, #{a => 1}}, {a, 1, <<"a">>},
                 {a, 1, [{b, 1} | foo]}, {a, 1, [x | foo]}, {a, 2, [{a} | foo]},
                 {a, 1, [{b, 1} | {a, 1}]}, {1, 2, [{a, 2} | foo]},
                 {1.0, 2, [{a, 2} | 7]}, {{k}, 1, [{{j}} | <<>>]},
                 {a, 1, Long ++ foo}])),
        %% BEAM checks the tail only when the walk reaches it, so a match
        %% before an improper tail is still found.
        lists:foreach(fun({Key, N, List, Hit}) ->
            Hit = Find(Key, N, List),
            true = Member(Key, N, List),
            {value, Hit} = Search(Key, N, List)
        end, id([{a, 1, [{a, 1} | foo], {a, 1}},
                 {b, 1, [{a, 1}, {b, 2} | foo], {b, 2}},
                 {1, 2, [{a, 1.0} | foo], {a, 1.0}},
                 {a, 1, Long ++ [{a, 1} | foo], {a, 1}}]))
    end, [direct, apply_call, dynamic, remote_fun]),
    io:format("lists_key_argument_errors_ok~n"),
    ok.

updates(direct) ->
    {fun(K, N, L, T) -> lists:keyreplace(K, N, L, T) end,
     fun(K, N, L, T) -> lists:keystore(K, N, L, T) end};
updates(apply_call) ->
    {fun(K, N, L, T) -> apply(id(lists), id(keyreplace), [K,N,L,T]) end,
     fun(K, N, L, T) -> apply(id(lists), id(keystore), [K,N,L,T]) end};
updates(dynamic) ->
    M = id(lists), R = id(keyreplace), S = id(keystore),
    {fun(K, N, L, T) -> M:R(K, N, L, T) end,
     fun(K, N, L, T) -> M:S(K, N, L, T) end};
updates(remote_fun) ->
    {fun lists:keyreplace/4, fun lists:keystore/4}.

searches(direct) ->
    {fun(K, N, L) -> lists:keyfind(K, N, L) end,
     fun(K, N, L) -> lists:keymember(K, N, L) end,
     fun(K, N, L) -> lists:keysearch(K, N, L) end};
searches(apply_call) ->
    {fun(K, N, L) -> apply(id(lists), id(keyfind), [K,N,L]) end,
     fun(K, N, L) -> apply(id(lists), id(keymember), [K,N,L]) end,
     fun(K, N, L) -> apply(id(lists), id(keysearch), [K,N,L]) end};
searches(dynamic) ->
    M = id(lists), F = id(keyfind), Mb = id(keymember), S = id(keysearch),
    {fun(K, N, L) -> M:F(K, N, L) end,
     fun(K, N, L) -> M:Mb(K, N, L) end,
     fun(K, N, L) -> M:S(K, N, L) end};
searches(remote_fun) ->
    {fun lists:keyfind/3, fun lists:keymember/3, fun lists:keysearch/3}.

failure(F) ->
    try F() of Value -> {unexpected_return,Value}
    catch error:Reason -> Reason
    end.

id(X) -> X.
