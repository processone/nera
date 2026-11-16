%% EXPECT: binary_split_options_ok
%% Regression: native binary:split ignored trim, scope and unknown options,
%% and rounded a partial-byte subject up to a binary. The CSV and streamed
%% line inputs below exercise the reported defects and BEAM's option rules.
%% Run this same fixture on native Nera and BEAM; reaching the marker is required.
-module(binary_split_options).
-export([test/0]).

test() ->
    %% trim removes only trailing empty fields; trim_all removes every empty
    %% field, even when trim is also present, independent of option order.
    lists:foreach(fun check/1,
        [{<<"a,b,,">>, <<",">>, [global, trim], [<<"a">>, <<"b">>]},
         {<<",a,,b,,">>, <<",">>, [trim, global], [<<>>, <<"a">>, <<>>, <<"b">>]},
         {<<"a,,">>, <<",">>, [trim], [<<"a">>, <<",">>]},
         {<<"a,">>, <<",">>, [trim], [<<"a">>]},
         {<<",">>, <<",">>, [trim], []},
         {<<>>, <<",">>, [], [<<>>]},
         {<<>>, <<",">>, [trim], []},
         {<<",,">>, <<",">>, [global, trim], []},
         {<<"a,b">>, <<";">>, [trim], [<<"a,b">>]},
         {<<",a,,b,,">>, <<",">>, [trim_all, trim, global], [<<"a">>, <<"b">>]},
         {<<",a,,b,,">>, <<",">>, [trim, global, trim_all], [<<"a">>, <<"b">>]},
         {<<"a,b,,">>, <<",">>, [global, global, trim, trim], [<<"a">>, <<"b">>]},
         %% BEAM accepts an improper tail after the final option cell.
         {<<"a,b,,">>, <<",">>, [global, trim | tail], [<<"a">>, <<"b">>]},
         %% A scope restricts matching, not the returned subject's prefix/suffix.
         {<<"a,b,c">>, <<",">>, [global, {scope, {2, 3}}], [<<"a,b">>, <<"c">>]},
         {<<"a,b,c">>, <<",">>, [{scope, {2, 3}}], [<<"a,b">>, <<"c">>]},
         {<<"a,b,c">>, <<",">>, [global, {scope, {5, -3}}], [<<"a,b">>, <<"c">>]},
         {<<"a,b,c">>, <<",">>, [global, {scope, {5, -5}}], [<<"a">>, <<"b">>, <<"c">>]},
         {<<"a,b,c">>, <<",">>, [{scope, {0, 0}}], [<<"a,b,c">>]},
         {<<"a,b,c">>, <<",">>, [{scope, {5, 0}}, trim], [<<"a,b,c">>]},
         {<<"a,b,c">>, <<",">>, [{scope, {2, 1}}], [<<"a,b,c">>]},
         {<<>>, <<",">>, [{scope, {0, 0}}, trim], []},
         {<<"a,b,c">>, <<",">>, [{scope, {0, 2}}, {scope, {2, 3}}, global],
          [<<"a,b">>, <<"c">>]},
         {<<"a,b,c">>, <<",">>, [{scope, {2, 3}}, {scope, {0, 2}}, global],
          [<<"a">>, <<"b,c">>]},
         {<<"a,b,,">>, <<",">>, [global, trim, {scope, {0, 4}}], [<<"a">>, <<"b">>, <<",">>]},
         %% Only whole matches inside the scope count; the longest eligible
         %% pattern wins when a longer pattern crosses the scope's end.
         {<<"one\r\ntwo\r\n">>, [<<"\r\n">>], [global, {scope, {0, 4}}],
          [<<"one\r\ntwo\r\n">>]},
         {<<"one\r\ntwo\r\n">>, [<<"\r\n">>, <<"\r">>], [global, {scope, {0, 4}}],
          [<<"one">>, <<"\ntwo\r\n">>]},
         {<<"one\r\ntwo\r\n">>, [<<"\r\n">>, <<"\r">>], [global, trim, {scope, {3, 7}}],
          [<<"one">>, <<"two">>]}]),

    %% A scope is relative to a sub-binary, not to its retained backing bytes.
    Line = <<"data: hi\r\n">>,
    Width = byte_size(Line),
    Stream = iolist_to_binary(lists:duplicate(100, Line)),
    Sub = binary:part(Stream, Width, 10 * Width),
    Prefix = iolist_to_binary([lists:duplicate(9, Line), <<"data: hi">>]),
    check({Sub, [<<"\r\n">>, <<"\n">>], [global, trim, {scope, {9 * Width, Width}}],
           [Prefix]}),
    <<_:3, Shifted:5/binary, _:5>> = id(<<0:3, "a,b,c", 0:5>>),
    check({Shifted, <<",">>, [global, {scope, {2, 3}}], [<<"a,b">>, <<"c">>]}),

    %% Unknown cells and malformed or out-of-range scopes raise badarg, even
    %% when no match is possible. Every scope is checked before the last wins.
    lists:foreach(fun(Options) ->
        lists:foreach(fun(S) -> check_badarg(S, <<",">>, Options) end,
                      [<<"a,b,c">>, <<>>])
    end, [[foo], [global, foo], [trim, 42], [{foo, {0, 1}}], [global, {trim}],
          foo, 42, {}, <<>>, [{scope}], [{scope, 1}], [{scope, {0}}],
          [{scope, {0, 1, 2}}], [{scope, {a, 1}}], [{scope, {0, 1.0}}],
          [{scope, {-1, 1}}], [{scope, {6, 0}}], [{scope, {0, 6}}],
          [{scope, {2, -3}}], [{scope, {0, 1 bsl 100}}],
          [{scope, {1 bsl 100, 0}}], [{scope, {0, -(1 bsl 100)}}],
          [{scope, {16#7fffffffffffffff, 1}}],
          [{scope, {99, 0}}, {scope, {0, 0}}],
          [{scope, {0, 0}}, {scope, {99, 0}}]]),
    lists:foreach(fun(S) ->
        check_badarg(S, <<"a">>, []),
        check_badarg(S, <<"a">>, [global, trim, {scope, {0, 0}}]),
        badarg = failure(fun() -> binary:split(id(S), <<"a">>) end),
        badarg = failure(fun() -> apply(binary, split, [S, <<"a">>]) end),
        badarg = failure(fun() -> (fun binary:split/2)(S, <<"a">>) end)
    end, [<<1:3>>, <<"data", 1:1>>, atom, 42, []]),
    io:format("binary_split_options_ok~n"),
    ok.

check({Subject, Pattern, Options, Expected}) ->
    Expected = binary:split(id(Subject), Pattern, Options),
    Expected = apply(binary, split, [Subject, Pattern, Options]),
    Expected = (fun binary:split/3)(Subject, Pattern, Options).

check_badarg(Subject, Pattern, Options) ->
    badarg = failure(fun() -> binary:split(id(Subject), Pattern, Options) end),
    badarg = failure(fun() -> apply(binary, split, [Subject, Pattern, Options]) end),
    badarg = failure(fun() -> (fun binary:split/3)(Subject, Pattern, Options) end).

failure(F) ->
    try F() of
        Value -> {unexpected_return, Value}
    catch
        error:Reason -> Reason
    end.

id(X) -> X.
