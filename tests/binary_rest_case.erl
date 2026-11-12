%% EXPECT: binary_rest_case_ok
-module(binary_rest_case).
-export([test/0]).

%% A branch must not consume the deferred binary-tail state of its siblings.
%% The second tail call used an undefined IR register before the fix.
test() ->
    42 = tail_branch(<<0,42>>, second),
    42 = tail_branch(<<0,42>>, first),
    <<42>> = value_branch(<<0,42>>, second),
    <<42>> = value_branch(<<0,42>>, first),
    {42, <<42>>} = after_branch(<<0,42>>, first),
    {0, <<42>>} = after_branch(<<0,42>>, second),
    42 = nested_branch(<<0,42>>, first, second),
    42 = nested_branch(<<0,42>>, second, first),
    io:format("binary_rest_case_ok~n"),
    ok.

tail_branch(<<_, Rest/binary>>, Which) ->
    case Which of
        first -> head(Rest);
        second -> head(Rest)
    end.

value_branch(<<_, Rest/binary>>, Which) ->
    case Which of
        first -> Rest;
        second -> Rest
    end.

after_branch(<<_, Rest/binary>>, Which) ->
    Value = case Which of
        first -> head(Rest);
        second -> 0
    end,
    {Value, Rest}.

nested_branch(<<_, Rest/binary>>, Outer, Inner) ->
    case Outer of
        first -> case Inner of
            first -> head(Rest);
            second -> head(Rest)
        end;
        second -> head(Rest)
    end.

head(<<X, _/binary>>) -> X.
