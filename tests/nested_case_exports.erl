%% EXPECT: nested_case_exports_ok
-module(nested_case_exports).
-export([test/0]).

%% Every case path supplies an outer input, not just an expression result.
test() ->
    lists:foreach(fun({Outer, Inner, Key, Width}) ->
        {Key, [42], 42, 42, true} = mixed(Outer, Inner),
        {Key, Width, 42} = nested(Outer, Inner),
        {Key, 42} = deep(Outer, Inner),
        {Key, true} = reused(Outer, Inner),
        {Key, 42} = wrapped(Outer, Inner)
    end, [
        {left, small, route, 8},
        {left, large, other, 16},
        {right, small, fallback, 8},
        {right, large, fallback, 16}
    ]),
    io:format("nested_case_exports_ok~n"),
    ok.

mixed(Outer, Inner) ->
    case Outer of
        left ->
            case Inner of
                small -> K = route;
                large -> K = other
            end;
        right -> K = fallback
    end,
    Map = #{K => 42},
    Read = fun(#{K := V}) -> V end,
    Capture = fun() -> K end,
    %% K must be an existing key input, while Value is fresh.
    Values = [Value || #{K := Value} <- [Map]],
    #{K := Direct} = Map,
    {K, Values, Read(Map), Direct, Capture() =:= K}.

nested(Outer, Inner) ->
    case Outer of
        left ->
            case Inner of
                small -> K = route, N = 8;
                large -> K = other, N = 16
            end;
        right ->
            case Inner of
                small -> K = fallback, N = 8;
                large -> K = fallback, N = 16
            end
    end,
    Read = fun(<<V:N>>) -> V end,
    [42] = [V || <<V:N>> <- [<<42:N>>]],
    {K, N, Read(<<42:N>>)}.

deep(Outer, Inner) ->
    case Outer of
        left ->
            case Inner of
                small ->
                    case id(Inner) of
                        small -> K = route;
                        _ -> K = unused
                    end;
                large -> K = other
            end;
        right -> K = fallback
    end,
    Read = fun(#{K := V}) -> V end,
    {K, Read(#{K => 42})}.

reused(Outer, Inner) ->
    case Outer of
        left ->
            case Inner of
                small -> K = route;
                large -> K = other
            end;
        right -> K = fallback
    end,
    %% Once exported, a partial later match compares with the bound K.
    case id(Inner) of
        small -> K = id(K);
        large -> ok
    end,
    {K, (fun() -> K end)() =:= K}.

wrapped(Outer, Inner) ->
    Result = case Outer of
        left ->
            begin
                case Inner of
                    small -> K = route;
                    large -> K = other
                end
            end;
        right -> K = fallback
    end,
    K = Result,
    Read = fun(#{K := V}) -> V end,
    {K, Read(#{K => 42})}.

id(Value) -> Value.
