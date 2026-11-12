%% Test: repeated variables in patterns (equality matching).
%% Verifies that:
%%  1. Repeated variable bindings correctly enforce equality constraints
%%  2. No false "unused variable" or "unreachable clause" warnings
-module(test_repeated_var_pattern).
-export([test/0]).

test() ->
    %% Repeated var across function args — equality constraint
    equal = match_pair(1, 1),
    different = match_pair(1, 2),

    %% Repeated var in tuple pattern (case)
    same = check_tuple({a, a}),
    diff = check_tuple({a, b}),

    %% Repeated var across record and tuple (simulated with tuple)
    ok = handle_info({tcp, my_socket, <<"data">>}, my_socket),
    error = handle_info({tcp, my_socket, <<"data">>}, other_socket),

    ok.

match_pair(X, X) -> equal;
match_pair(_, _) -> different.

check_tuple(Input) ->
    case Input of
        {X, X} -> same;
        _ -> diff
    end.

handle_info({tcp, Socket, _Data}, Socket) -> ok;
handle_info(_, _) -> error.

id(X) -> X.
