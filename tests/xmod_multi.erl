-module(xmod_multi).
-export([test/0]).
%% MODULES: _xmod_math.erl _xmod_string_utils.erl

%% Test cross-module calls to MULTIPLE helper modules simultaneously.
%% Verifies that direct linking works when several modules are co-compiled.

test() ->
    %% Calls to xmod_math
    42 = xmod_math:double(21),
    7  = xmod_math:add(3, 4),

    %% Calls to xmod_string_utils
    [3, 2, 1] = xmod_string_utils:reverse([1, 2, 3]),
    3 = xmod_string_utils:length_of([a, b, c]),
    [] = xmod_string_utils:reverse([]),
    0  = xmod_string_utils:length_of([]),

    ok.
