-module(large_list).
-export([test/0]).

%% Regression test: large lists must not crash the GC copier or
%% message deep-copy (both used recursive cons-tail traversal that
%% caused Rust stack overflow at ~36K elements).

test() ->
    %% 1. lists:duplicate — tail-recursive stdlib + GC copying
    L1 = lists:duplicate(100000, $a),
    B1 = list_to_binary(L1),
    100000 = byte_size(B1),

    %% 2. lists:seq — tail-recursive stdlib + GC copying
    L2 = lists:seq(1, 100000),
    100000 = length(L2),
    1 = hd(L2),

    %% 3. Large list sent between processes (exercises message deep-copy)
    Self = self(),
    spawn(fun() ->
        L = lists:duplicate(100000, $b),
        Self ! {done, list_to_binary(L)}
    end),
    receive
        {done, B2} ->
            100000 = byte_size(B2),
            ok
    end,

    ok.
