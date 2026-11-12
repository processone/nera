%% EXPECT: try_catch_bound_vars_ok
%% NERA-45: try-of and catch patterns compare already-bound names with =:=,
%% rather than rebinding them. All expected results are also checked on BEAM.
-module(try_catch_bound_vars).
-export([test/0]).

test() ->
    %% The original four calls must print [matched,other,matched,other].
    lists:foreach(fun check/1, [
        {fun try_of/2, [1, 1], matched},
        {fun try_of/2, [1, 2], other},
        {fun catch_throw/2, [1, 1], matched},
        {fun catch_throw/2, [1, 2], other}
    ]),
    %% A plain variable used to bypass catch's pattern compiler entirely.
    %% Check known, wildcard and named classes, with and without guards.
    lists:foreach(fun(Fun) ->
        lists:foreach(fun({X, V, Expected}) -> check({Fun, [X, V], Expected}) end, [
            {1, 1, matched}, {1, 2, other}, {1, 1.0, other},
            {1.0, 1, other}, {1.0, 1.0, matched},
            {hello, hello, matched}, {hello, goodbye, other},
            {{a, [1]}, {a, [1]}, matched}, {{a, [1]}, {a, [2]}, other},
            {<<"ab">>, <<"ab">>, matched}, {<<"ab">>, <<"ac">>, other}
        ])
    end, [fun try_of/2, fun catch_throw/2, fun catch_any/2,
          fun catch_named/2, fun catch_guarded/2]),
    lists:foreach(fun check/1, [
        %% Compound patterns must compare outer names and bind fresh names.
        {fun try_tuple/2, [7, {7, tail}], {matched, 7, tail}},
        {fun try_tuple/2, [7, {8, tail}], other},
        {fun catch_tuple/2, [7, {7, tail}], {matched, 7, tail}},
        {fun catch_tuple/2, [7, {8, tail}], other},
        {fun catch_named_tuple/2, [7, {7, tail}], {matched, 7, tail}},
        {fun catch_named_tuple/2, [7, {8, tail}], other},
        {fun try_map/2, [7, #{key => 7}], matched},
        {fun try_map/2, [7, #{key => 8}], other},
        {fun catch_map/2, [7, #{key => 7}], matched},
        {fun catch_map/2, [7, #{key => 8}], other},
        %% Binary segments must not overwrite a plain outer term.
        {fun try_byte/2, [97, <<"a">>], {matched, 97}},
        {fun try_byte/2, [97, <<"b">>], {other, 97}},
        {fun try_byte/2, [97.0, <<"a">>], {other, 97.0}},
        {fun catch_byte/2, [97, <<"a">>], {matched, 97}},
        {fun catch_byte/2, [97, <<"b">>], {other, 97}},
        {fun catch_utf/2, [233, <<233/utf8>>], {matched, 233}},
        {fun catch_utf/2, [233, <<234/utf8>>], {other, 233}},
        %% Bound deferred tails: fixed-size and rest segments, and bare names.
        {fun try_tail/2, [<<0, "a">>, <<"a">>], {matched, <<"a">>}},
        {fun try_tail/2, [<<0, "a">>, <<"b">>], {other, <<"a">>}},
        {fun try_tail/2, [<<0, "a">>, nope], {other, <<"a">>}},
        {fun catch_tail/2, [<<0, "a">>, <<"a">>], {matched, <<"a">>}},
        {fun catch_tail/2, [<<0, "a">>, <<"b">>], {other, <<"a">>}},
        {fun catch_named_tail/2, [<<0, "a">>, <<"a">>], {matched, <<"a">>}},
        {fun catch_named_tail/2, [<<0, "a">>, <<"b">>], {other, <<"a">>}},
        {fun try_rest/2, [<<0, "ab">>, <<1, "ab">>], {matched, <<"ab">>}},
        {fun try_rest/2, [<<0, "ab">>, <<1, "ac">>], {other, <<"ab">>}},
        {fun catch_rest/2, [<<0, "ab">>, <<1, "ab">>], {matched, <<"ab">>}},
        {fun catch_rest/2, [<<0, "ab">>, <<1, "ac">>], {other, <<"ab">>}},
        {fun catch_bare_tail/2, [<<0, "ab">>, <<"ab">>], {matched, <<"ab">>}},
        {fun catch_bare_tail/2, [<<0, "ab">>, <<"ac">>], {other, <<"ab">>}},
        %% Class and reason form one pattern: outer and repeated class names.
        {fun bound_class/2, [throw, value], {throw, value}},
        {fun bound_class/2, [error, value], other},
        {fun repeated_class/1, [throw], matched},
        {fun repeated_class/1, [value], other},
        {fun fresh/1, [value], {throw, value}},
        {fun fresh_tuple/1, [{7, tail}], {throw, 7, tail}},
        {fun class_fallthrough/1, [value], {throw, value}},
        {fun catch_error/2, [reason, reason], matched},
        {fun catch_error/2, [reason, different], other},
        {fun catch_exit/2, [reason, reason], matched},
        {fun catch_exit/2, [reason, different], other},
        {fun catch_guarded/2, [blocked, blocked], other},
        {fun catch_stack/2, [reason, reason], matched},
        {fun catch_stack/2, [reason, different], other},
        {fun reraise/2, [1, 2], {raised, throw, 2}},
        {fun body_binding/1, [value], other},
        {fun after_binding/2, [1, 2], {other, 1}}
    ]),
    io:format("try_catch_bound_vars_ok~n"),
    ok.

check({Fun, Args, Expected}) ->
    Actual = try apply(Fun, Args) catch Class:Reason -> {raised, Class, Reason} end,
    case Actual of
        Expected -> ok;
        _ -> erlang:error({mismatch, Fun, Args, Expected, Actual})
    end.

try_of(X, V) -> try V of X -> matched; _ -> other catch _:_ -> caught end.
catch_throw(X, V) -> try throw(V) catch throw:X -> matched; throw:_ -> other end.
catch_any(X, V) -> try throw(V) catch _:X -> matched; _:_ -> other end.
catch_named(X, V) -> try throw(V) catch Class:X -> Class = throw, matched; _:_ -> other end.
catch_guarded(X, V) ->
    try throw(V) catch throw:X when X =/= blocked -> matched; throw:_ -> other end.
try_tuple(X, V) -> try V of {X, Y} -> {matched, X, Y}; _ -> other catch _:_ -> caught end.
catch_tuple(X, V) -> try throw(V) catch throw:{X, Y} -> {matched, X, Y}; _:_ -> other end.
catch_named_tuple(X, V) ->
    try throw(V) catch Class:{X, Y} when Class =:= throw -> {matched, X, Y}; _:_ -> other end.
try_map(X, V) -> try V of #{key := X} -> matched; _ -> other catch _:_ -> caught end.
catch_map(X, V) -> try throw(V) catch throw:#{key := X} -> matched; _:_ -> other end.
try_byte(X, V) ->
    R = try V of <<X:8>> -> matched; _ -> other catch _:_ -> caught end, {R, X}.
catch_byte(X, V) ->
    R = try throw(V) catch throw:<<X:8>> -> matched; _:_ -> other end, {R, X}.
catch_utf(X, V) ->
    R = try throw(V) catch _:<<X/utf8>> -> matched; _:_ -> other end, {R, X}.
try_tail(<<0, X/binary>>, V) ->
    R = try V of <<X:1/binary>> -> matched; _ -> other catch _:_ -> caught end, {R, X}.
catch_tail(<<0, X/binary>>, V) ->
    R = try throw(V) catch throw:<<X:1/binary>> -> matched; _:_ -> other end, {R, X}.
catch_named_tail(<<0, X/binary>>, V) ->
    R = try throw(V) catch Class:<<X:1/binary>> when Class =:= throw -> matched; _:_ -> other end,
    {R, X}.
try_rest(<<0, X/binary>>, V) ->
    R = try V of <<_:8, X/binary>> -> matched; _ -> other catch _:_ -> caught end, {R, X}.
catch_rest(<<0, X/binary>>, V) ->
    R = try throw(V) catch _:<<_:8, X/binary>> -> matched; _:_ -> other end, {R, X}.
catch_bare_tail(<<0, X/binary>>, V) ->
    R = try throw(V) catch throw:X -> matched; _:_ -> other end, {R, X}.
bound_class(Class, V) -> try throw(V) catch Class:R -> {Class, R}; _:_ -> other end.
repeated_class(V) -> try throw(V) catch Class:Class -> matched; _:_ -> other end.
fresh(V) -> try throw(V) catch Class:R when Class =:= throw -> {Class, R} end.
fresh_tuple(V) -> try throw(V) catch Class:{X, Y} -> {Class, X, Y} end.
class_fallthrough(V) ->
    try throw(V) catch error:V -> wrong; Class:R -> {Class, R} end.
catch_error(X, V) -> try erlang:error(V) catch error:X -> matched; error:_ -> other end.
catch_exit(X, V) -> try exit(V) catch exit:X -> matched; exit:_ -> other end.
catch_stack(X, V) ->
    try throw(V) catch throw:X:Stack -> true = is_list(Stack), matched; _:_ -> other end.
reraise(X, V) -> try throw(V) catch throw:X -> matched end.
%% X is bound by the body, but the body result is {X}, not X. A matcher
%% treating the of-pattern X as fresh would incorrectly return matched.
body_binding(V) -> try X = V, {X} of X -> matched; _ -> other catch _:_ -> caught end.
after_binding(X, V) ->
    R = try V of X -> matched; _ -> other after X = X end, {R, X}.
