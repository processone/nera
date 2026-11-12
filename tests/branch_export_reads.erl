%% EXPECT: branch_export_reads_ok
%% Regression: a variable bound in a begin...end block, or in every branch of
%% a case, if or receive, stays bound after the expression, as in BEAM. Later
%% code reads it in bodies, patterns, guards, fun captures, map keys and
%% binary sizes, record field defaults included. Before the fix, ordinary
%% reads failed to compile with "undefined variable", and fun-head or
%% generator keys and sizes read a nil key or used an 8-bit size instead.
%% Code with its own scope, such as a comprehension's first generator source,
%% leaves a binary tail it reads readable afterwards. The same fixture runs
%% on BEAM.
-module(branch_export_reads).
-export([test/0]).

%% Each default's fun exports a case binding to its own later code.
-record(defaults, {
    read = fun(M) ->
        case M of a -> Y = 1; _ -> Y = 2 end,
        Y
    end,
    key = fun(M) ->
        case M of 1 -> K = one; _ -> K = two end,
        G = fun(#{K := V}) -> V end,
        G(#{one => 1, two => 2})
    end,
    size = fun(W) ->
        case W of 1 -> N = 16; _ -> N = 8 end,
        [V || <<V:N>> <= <<1, 2, 3, 4>>]
    end
}).

%% The checks sit in small group functions: one large function makes the IR
%% verifier in the unit tests too slow.
test() ->
    construct_reads(),
    context_reads(),
    fun_head_inputs(),
    generator_inputs(),
    other_inputs(),
    later_patterns(),
    binary_tails(),
    fresh_bindings(),
    local_scope_tails(),
    record_default_exports(),
    rebinding_heads(),
    copied_code_reads(),
    continuation_reads(),
    io:format("branch_export_reads_ok~n"),
    ok.

%% Ordinary body reads after each construct.
construct_reads() ->
    {block, 1} = block_read(id(1)),
    {a, a} = case_read(id(1)),
    {b, b} = case_read(id(2)),
    {tagged, x} = case_pattern_read(id({a, x})),
    {tagged, y} = case_pattern_read(id({b, y})),
    {small, 1} = if_read(id(1)),
    {large, 50} = if_read(id(100)),
    {got, 7} = receive_read(id({value, 7})),
    {got, route} = receive_read(id(none)),
    {waited, 3} = receive_after_only(id(3)),
    {nested, inner} = nested_read(id(1), id(1)),
    {nested, other} = nested_read(id(1), id(2)),
    {nested, block} = nested_read(id(2), id(1)),
    ok.

%% Body reads of exports from expressions inside other expressions.
context_reads() ->
    {scrutinee, a} = scrutinee_read(id(1)),
    {argument, b} = call_argument_read(id(2)),
    {try_of, a} = try_of_read(id(1)),
    {try_of, b} = try_of_read(id(2)),
    {raised, b} = raising_branch_read(id(2)),
    ok.

%% Fun-head map keys and binary sizes after the expression.
fun_head_inputs() ->
    route_value = fun_key(id(route), id(#{route => route_value, other => other_value})),
    other_value = fun_key(id(other), id(#{route => route_value, other => other_value})),
    {wide, 16#0102} = fun_size(id(wide), id(<<1, 2>>)),
    {narrow, 1} = fun_size(id(narrow), id(<<1>>)),
    {block_size, 16#0102} = block_fun_size(id(<<1, 2>>)),
    ok.

%% Generator map keys and binary sizes after the expression.
generator_inputs() ->
    [v1, v2] = generator_key(id(1), id([#{k1 => v1}, #{k1 => v2}, #{k2 => v3}])),
    [v3] = generator_key(id(2), id([#{k1 => v1}, #{k2 => v3}])),
    [16#0102, 16#0304] = generator_size(id(true), id([{x, <<1, 2>>}, {x, <<3, 4>>}])),
    [1, 3] = generator_size(id(false), id([{x, <<1>>}, {x, <<3>>}])),
    [16#0102, 16#0304] = bin_generator_size(id(receive_wide), id(<<1, 2, 3, 4>>)),
    ok.

%% Construction sizes, guards and fun captures after the expression.
other_inputs() ->
    <<0, 5>> = construction_size(id(1), id(5)),
    {guard, big} = guard_read(id(20), id(1)),
    {guard, small} = guard_read(id(5), id(1)),
    {captured, 1} = (fun_capture(id(1)))(),
    ok.

%% Later patterns compare against the exported value.
later_patterns() ->
    same = later_case_pattern(id(1), id({a})),
    different = later_case_pattern(id(1), id({b})),
    {matched, a} = later_match(id(1), id(a)),
    {badmatch, b} = catch_badmatch(fun() -> later_match(id(1), id(b)) end),
    {lhs_size, 16#0102} = match_lhs_size(id(1)),
    ok.

%% Binary tails bound in every branch.
binary_tails() ->
    {tail, <<"abc">>} = tail_read(id(<<1, "abc">>)),
    {tail, <<"xyz">>} = tail_read(id(<<2, "xyz">>)),
    {tail, <<"abc">>} = tail_read_in_one_branch(id(<<1, "abc">>)),
    {tail, <<"xyz">>} = tail_read_in_one_branch(id(<<2, "xyz">>)),
    same_tail = tail_compare(id(<<1, "abc">>), id(<<"abc">>)),
    other_tail = tail_compare(id(<<2, "abc">>), id(<<"xyz">>)),
    {tail_size, 3} = tail_byte_size(id(<<1, "abc">>)),
    ok.

%% Fun heads and generators still bind fresh values with the same name.
fresh_bindings() ->
    {shadowed, other} = fun_head_shadows(id(1), id(other)),
    {generated, [x, y]} = generator_shadows(id(1), id([x, y])),

    %% A comprehension's first generator source keeps its bindings local:
    %% a later match of the same name binds a fresh value.
    {[1], 2} = generator_source_block(id([1]), id(2)),
    {[1], 2} = generator_source_case(id(1), id(2)),
    {[1], 2} = generator_source_match(id([1]), id(2)),
    ok.

%% Code with its own scope reads a binary tail bound before it, and the code
%% after it still reads that tail.
local_scope_tails() ->
    {[1, 2], <<1, 2>>} = list_source_tail(id(<<0, 1, 2>>)),
    {<<2, 3>>, <<1, 2>>} = bin_source_tail(id(<<0, 1, 2>>)),
    {[1, 2], <<1, 2>>} = list_generator_source_tail(id(<<0, 1, 2>>)),
    <<1, 2>> = (source_tail_capture(id(<<0, 1, 2>>)))(),
    same_tail = source_tail_compare(id(<<0, 1, 2>>), id(<<1, 2>>)),
    other_tail = source_tail_compare(id(<<0, 1, 2>>), id(<<3>>)),
    {maybe_tail, <<1, 2>>} = maybe_tail(id(<<0, 1, 2>>)),
    ok.

%% Record field defaults export their own branch bindings.
record_default_exports() ->
    #defaults{read = Read, key = Key, size = Size} = id(#defaults{}),
    {1, 2} = {Read(id(a)), Read(id(b))},
    {1, 2} = {Key(id(1)), Key(id(2))},
    {[16#0102, 16#0304], [1, 2, 3, 4]} = {Size(id(1)), Size(id(2))},
    ok.

%% A fun head or generator that rebinds a name elsewhere does not change
%% what the exported name reads, and the keys and sizes of a head that
%% rebinds the exported name still read it.
rebinding_heads() ->
    {x, [y], <<5>>} = unrelated_then_read(id(<<1, 5>>)),
    {x, [y], <<6, 7>>} = unrelated_then_read(id(<<2, 6, 7>>)),
    {ok, <<5>>} = fun_local_then_read(id(<<1, 5>>)),
    {x, 16#0102} = head_size_reads_export(id(1), id(<<1, 2>>)),
    [{x, 16#0102}] = generator_size_reads_export(id(1), id([{x, <<1, 2>>}])),
    ok.

%% A generator source and a catch operand read their own case exports.
copied_code_reads() ->
    [1, 2] = source_case_tail_read(id(<<0, 1, 2>>)),
    [1, 2] = catch_case_tail_read(id(<<0, 1, 2>>)),
    ok.

%% A case export is read by the code after it, while code that does not see
%% it reuses the name: a sibling clause, another fun clause and a receive
%% timeout.
continuation_reads() ->
    {a, <<5>>} = sibling_clause_read(id(a), id(<<1, 5>>)),
    {b, other} = sibling_clause_read(id(b), id(<<1, 5>>)),
    {<<5>>, none} = fun_clause_read(id(<<1, 5>>)),
    <<5>> = timeout_local_read(id(<<1, 5>>)),
    ok.

block_read(X) ->
    begin Y = X end,
    {block, Y}.

case_read(Mode) ->
    case Mode of
        1 -> Y = a;
        _ -> Y = b
    end,
    {Y, Y}.

case_pattern_read(T) ->
    case T of
        {a, Y} -> ok;
        {b, Y} -> ok
    end,
    {tagged, Y}.

if_read(N) ->
    if
        N < 10 -> Size = small, Y = N;
        true -> Size = large, Y = N div 2
    end,
    {Size, Y}.

receive_read(Message) ->
    case Message of
        none -> ok;
        _ -> self() ! Message
    end,
    receive
        {value, Y} -> Tag = got
    after 0 ->
        Tag = got,
        Y = route
    end,
    {Tag, Y}.

receive_after_only(N) ->
    receive
    after 0 -> Y = N
    end,
    {waited, Y}.

nested_read(A, B) ->
    case A of
        1 ->
            case B of
                1 -> Y = inner;
                _ -> begin Y = other end
            end;
        _ ->
            begin Y = block end
    end,
    {nested, Y}.

scrutinee_read(Mode) ->
    case (case Mode of 1 -> Y = a; _ -> Y = b end) of
        _ -> {scrutinee, Y}
    end.

call_argument_read(Mode) ->
    _ = id(case Mode of 1 -> Y = a; _ -> Y = b end),
    {argument, Y}.

try_of_read(Mode) ->
    try case Mode of 1 -> Y = a; _ -> Y = b end of
        _ -> {try_of, Y}
    catch
        _:_ -> failed
    end.

raising_branch_read(Mode) ->
    try
        case Mode of
            1 -> Y = a, erlang:error(raised);
            _ -> Y = b
        end,
        {raised, Y}
    catch
        error:raised -> raised
    end.

fun_key(Which, Map) ->
    case Which of
        route -> K = route;
        _ -> K = other
    end,
    F = fun(#{K := V}) -> V end,
    F(Map).

fun_size(Mode, Bin) ->
    if
        Mode =:= wide -> N = 16;
        true -> N = 8
    end,
    F = fun(<<V:N>>) -> {Mode, V} end,
    F(Bin).

block_fun_size(Bin) ->
    begin N = 16 end,
    F = fun(<<V:N>>) -> {block_size, V} end,
    F(Bin).

generator_key(Mode, Maps) ->
    case Mode of
        1 -> K = k1;
        _ -> K = k2
    end,
    [V || #{K := V} <- Maps].

generator_size(Wide, Pairs) ->
    if
        Wide -> N = 16;
        true -> N = 8
    end,
    [V || {x, <<V:N>>} <- Pairs].

bin_generator_size(Mode, Bin) ->
    self() ! {width, Mode},
    receive
        {width, receive_wide} -> N = 16;
        {width, _} -> N = 8
    end,
    [V || <<V:N>> <= Bin].

construction_size(Mode, V) ->
    case Mode of
        1 -> N = 16;
        _ -> N = 8
    end,
    <<V:N>>.

guard_read(X, Mode) ->
    case Mode of
        1 -> Limit = 10;
        _ -> Limit = 100
    end,
    case X of
        Big when Big > Limit -> {guard, big};
        _ -> {guard, small}
    end.

fun_capture(Mode) ->
    case Mode of
        1 -> Y = 1;
        _ -> Y = 2
    end,
    fun() -> {captured, Y} end.

later_case_pattern(Mode, T) ->
    case Mode of
        1 -> Y = a;
        _ -> Y = b
    end,
    case T of
        {Y} -> same;
        _ -> different
    end.

later_match(Mode, X) ->
    case Mode of
        1 -> Y = a;
        _ -> Y = b
    end,
    Y = X,
    {matched, Y}.

catch_badmatch(F) ->
    try F() of
        Result -> {no_error, Result}
    catch
        error:{badmatch, V} -> {badmatch, V}
    end.

match_lhs_size(Mode) ->
    <<A:Y>> = case Mode of
        1 -> Y = 16, id(<<1, 2>>);
        _ -> Y = 8, id(<<3>>)
    end,
    {lhs_size, A}.

tail_read(Bin) ->
    case Bin of
        <<1, Rest/binary>> -> ok;
        <<2, Rest/binary>> -> ok
    end,
    {tail, Rest}.

tail_read_in_one_branch(Bin) ->
    case Bin of
        <<1, Rest/binary>> -> _ = id(Rest), ok;
        <<2, Rest/binary>> -> ok
    end,
    {tail, Rest}.

tail_compare(Bin, Other) ->
    case Bin of
        <<1, Rest/binary>> -> ok;
        <<_, Rest/binary>> -> ok
    end,
    case Other of
        Rest -> same_tail;
        _ -> other_tail
    end.

tail_byte_size(Bin) ->
    case Bin of
        <<1, Rest/binary>> -> ok;
        <<_, Rest/binary>> -> ok
    end,
    {tail_size, byte_size(Rest)}.

fun_head_shadows(Mode, Arg) ->
    case Mode of
        1 -> K = route;
        _ -> K = other_route
    end,
    F = fun(K) -> K end,
    {shadowed, F(Arg)}.

generator_shadows(Mode, L) ->
    case Mode of
        1 -> K = route;
        _ -> K = other_route
    end,
    {generated, [K || K <- L]}.

generator_source_block(L, V) ->
    R = [X || X <- begin Y = L, Y end],
    Y = V,
    {R, Y}.

generator_source_case(Mode, V) ->
    R = [X || X <- case Mode of 1 -> Y = [1]; _ -> Y = [2] end],
    Y = V,
    {R, Y}.

generator_source_match(L, V) ->
    R = [X || X <- (Y = L)],
    Y = V,
    {R, Y}.

list_source_tail(<<_, Rest/binary>>) ->
    R = [X || <<X>> <= Rest],
    {R, Rest}.

bin_source_tail(<<_, Rest/binary>>) ->
    R = << <<(X + 1)>> || <<X>> <= Rest >>,
    {R, Rest}.

list_generator_source_tail(<<_, Rest/binary>>) ->
    R = [X || X <- binary_to_list(Rest)],
    {R, Rest}.

source_tail_capture(<<_, Rest/binary>>) ->
    _ = [X || <<X>> <= Rest],
    fun() -> Rest end.

source_tail_compare(<<_, Rest/binary>>, Other) ->
    _ = [X || <<X>> <= Rest],
    case Other of
        Rest -> same_tail;
        _ -> other_tail
    end.

maybe_tail(<<_, Rest/binary>>) ->
    maybe
        id(Rest),
        ok
    end,
    {maybe_tail, Rest}.

unrelated_then_read(Bin) ->
    F = fun(Rest) -> Rest end,
    case Bin of
        <<1, Rest/binary>> -> ok;
        <<_, Rest/binary>> -> ok
    end,
    {F(x), [Rest || Rest <- [y]], Rest}.

fun_local_then_read(Bin) ->
    F = fun() -> Rest = id(ok), Rest end,
    case Bin of
        <<1, Rest/binary>> -> ok;
        <<_, Rest/binary>> -> ok
    end,
    {F(), Rest}.

source_case_tail_read(Bin) ->
    [X || X <- begin
                   case Bin of
                       <<1, Rest/binary>> -> ok;
                       <<_, Rest/binary>> -> ok
                   end,
                   binary_to_list(Rest)
               end].

catch_case_tail_read(Bin) ->
    catch begin
        case Bin of
            <<1, Rest/binary>> -> ok;
            <<_, Rest/binary>> -> ok
        end,
        binary_to_list(Rest)
    end.

sibling_clause_read(Mode, Bin) ->
    case Mode of
        a ->
            case Bin of
                <<1, Rest/binary>> -> ok;
                <<_, Rest/binary>> -> ok
            end,
            {a, Rest};
        _ ->
            Rest = other,
            {b, Rest}
    end.

fun_clause_read(Bin) ->
    F = fun(a) ->
                case Bin of
                    <<1, Rest/binary>> -> ok;
                    <<_, Rest/binary>> -> ok
                end,
                Rest;
           (_) ->
                Rest = none,
                Rest
        end,
    {F(a), F(b)}.

timeout_local_read(Bin) ->
    self() ! go,
    receive
        go ->
            case Bin of
                <<1, Rest/binary>> -> ok;
                <<_, Rest/binary>> -> ok
            end,
            Rest
    after (fun() -> Rest = 0, Rest end)() ->
        none
    end.

head_size_reads_export(Mode, Bin) ->
    case Mode of
        1 -> N = 16;
        _ -> N = 8
    end,
    F = fun(N, <<V:N>>) -> {N, V} end,
    F(x, Bin).

generator_size_reads_export(Mode, Pairs) ->
    case Mode of
        1 -> N = 16;
        _ -> N = 8
    end,
    [{N, V} || {N, <<V:N>>} <- Pairs].

id(X) -> X.
