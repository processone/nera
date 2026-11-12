-module(xmod_basic).
-export([test/0]).
%% MODULES: _xmod_math.erl

%% Test basic cross-module function calls (direct linking).
%% Calls functions in xmod_math module compiled together.

test() ->
    42 = xmod_math:double(21),
    15 = xmod_math:add(10, 5),
    15 = xmod_math:sum_list([1, 2, 3, 4, 5]),
    0  = xmod_math:sum_list([]),
    ok.
