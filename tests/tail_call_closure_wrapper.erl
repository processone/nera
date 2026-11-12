%% Regression test for tail-call trampoline resolution in local calls.
%%
%% When a local function tail-calls a closure (wrap(S, E) -> E(S)),
%% it returns via the trampoline: sets STATUS_TAIL_CALL and returns NIL.
%% The caller must resolve the pending tail call and use ctx.result
%% instead of the dummy NIL return value.
%%
%% Without the fix, list construction with wrapper functions that
%% tail-call closures produces truncated/corrupted results.
-module(tail_call_closure_wrapper).
-export([test/0]).

test() ->
    F = fun(X) -> X end,

    %% ── Basic: wrapper tail-calls closure, result used in list ──
    [a, b, stop] = build([a, b], F),

    %% ── Standalone call through wrapper ──
    a = wrap(a, F),
    hello = wrap(hello, F),
    42 = wrap(42, F),

    %% ── Wrapper with arity-3 closure (like thoas escape_json) ──
    G = fun(X, _Y, _Z) -> X end,
    abc = wrap3(abc, G),
    [abc, def, stop] = build3([abc, def], G),

    %% ── Multi-element list with wrapper calls ──
    [pre, a, mid, 1, sep, b, mid, 2, stop] = build_full([{a, 1}, {b, 2}], F),

    %% ── Wrapper returning a list (like thoas escape_json returns [Acc|Part]) ──
    H = fun(X) -> [prefix, X] end,
    [prefix, hello] = wrap(hello, H),
    [[prefix, a], [prefix, b], stop] = build([a, b], H),

    %% ── Nested: wrapper in map_naive-like pattern ──
    R = encode_map([{<<"a">>, 1}, {<<"b">>, 2}]),
    <<"{\"a\":1,\"b\":2}">> = iolist_to_binary(R),

    %% ── 3+ elements (multiple loop iterations) ──
    [a, b, c, d, stop] = build([a, b, c, d], F),

    %% ── Single element (no loop iteration) ──
    [a, stop] = build([a], F),

    %% ── Empty list ──
    [stop] = build([], F),

    %% ── Wrapper result used in arithmetic ──
    Inc = fun(X) -> X + 1 end,
    12 = wrap(10, Inc) + 1,

    %% ── Wrapper result used in pattern match ──
    {ok, value} = wrap({ok, value}, F),

    %% ── Wrapper result used in case expression ──
    yes = case wrap(true, F) of true -> yes; _ -> no end,
    no = case wrap(false, F) of true -> yes; _ -> no end,

    %% ── Wrapper result used as argument to another call ──
    3 = length(wrap([a, b, c], F)),
    <<"abc">> = list_to_binary(wrap([97, 98, 99], F)),

    %% ── Chained wrappers: wrap calls wrap2 which calls closure ──
    42 = chain_wrap(42, F),
    [a, b, stop] = build_chain([a, b], F),

    %% ── Wrapper that transforms before tail-calling closure ──
    Double = fun(X) -> X * 2 end,
    20 = wrap(10, Double),
    [2, 4, 6, stop] = build([1, 2, 3], Double),

    %% ── Closure capturing variables ──
    Base = 100,
    Add = fun(X) -> X + Base end,
    142 = wrap(42, Add),
    [101, 102, stop] = build([1, 2], Add),

    %% ── fun M:F/A remote fun ref through wrapper ──
    Abs = fun erlang:abs/1,
    42 = wrap(-42, Abs),
    [1, 2, 3, stop] = build([-1, -2, -3], Abs),

    %% ── Wrapper in try/catch ──
    Err = fun(_) -> error(boom) end,
    caught = try wrap(x, Err) catch error:boom -> caught end,

    %% ── Wrapper result used in guard ──
    yes = guard_check(wrap(42, F)),
    no = guard_check(wrap(hello, F)),

    %% ── Wrapper result in map construction ──
    M = #{key => wrap(value, F)},
    value = maps:get(key, M),

    %% ── Wrapper result as map key ──
    MK = maps:put(wrap(<<"k">>, F), 1, #{}),
    1 = maps:get(<<"k">>, MK),

    %% ── Wrapper result in binary construction ──
    Bin = <<(wrap(65, F))>>,
    <<"A">> = Bin,

    %% ── Multiple wrappers in same expression ──
    {a, b} = {wrap(a, F), wrap(b, F)},
    [a, b] = [wrap(a, F), wrap(b, F)],

    io:format("ok~n"),
    ok.

wrap(S, E) -> E(S).

wrap3(S, E) -> E(S, S, 0).

%% Chained: chain_wrap calls wrap2 which calls closure
chain_wrap(S, E) -> wrap2(S, E).
wrap2(S, E) -> E(S).

build([], _F) -> [stop];
build([H | T], F) -> [wrap(H, F) | build(T, F)].

build3([H | T], F) -> [wrap3(H, F) | loop3(T, F)].
loop3([], _F) -> [stop];
loop3([H | T], F) -> [wrap3(H, F) | loop3(T, F)].

build_full([{K, V} | T], F) ->
    [pre, wrap(K, F), mid, V | loop_full(T, F)].
loop_full([], _F) -> [stop];
loop_full([{K, V} | T], F) ->
    [sep, wrap(K, F), mid, V | loop_full(T, F)].

build_chain([H | T], F) -> [chain_wrap(H, F) | loop_chain(T, F)].
loop_chain([], _F) -> [stop];
loop_chain([H | T], F) -> [chain_wrap(H, F) | loop_chain(T, F)].

%% Mini thoas-like encode
encode_map(KVList) ->
    Escape = fun escape/3,
    map_naive(KVList, Escape).

map_naive([{Key, Value} | Tail], Escape) ->
    [<<"{\"">>,
     key(Key, Escape),
     <<"\":">>,
     integer_to_list(Value) |
     map_naive_loop(Tail, Escape)].

map_naive_loop([], _Escape) ->
    [$}];
map_naive_loop([{Key, Value} | Tail], Escape) ->
    [<<",\"">>,
     key(Key, Escape),
     <<"\":">>,
     integer_to_list(Value) |
     map_naive_loop(Tail, Escape)].

key(String, Escape) when is_binary(String) ->
    Escape(String, String, 0).

escape(Data, Input, Skip) ->
    escape_loop(Data, Input, Skip, 0).
escape_loop(<<_, Rest/binary>>, Input, Skip, Len) ->
    escape_loop(Rest, Input, Skip, Len + 1);
escape_loop(<<>>, Input, Skip, Len) ->
    binary_part(Input, Skip, Len).

guard_check(V) when is_integer(V) -> yes;
guard_check(_) -> no.
