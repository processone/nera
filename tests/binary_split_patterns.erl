%% EXPECT: binary_split_patterns_ok
%% Regression: binary:split/2,3 accept a list of patterns, as on BEAM. Each
%% split is at the earliest match, the longest pattern at that offset, and
%% global splits do not overlap. Before the fix, every list raised badarg
%% natively, so Cowlib's cow_sse:parse/2 failed on <<"data: hi\n\n">>.
%% The data below is the server-sent events input Cowlib splits with
%% [<<"\r\n">>, <<"\r">>, <<"\n">>] and [<<":\s">>, <<":">>].
%% Run this same fixture on native Nera and BEAM.
-module(binary_split_patterns).
-export([test/0]).

-define(EOL, [<<"\r\n">>, <<"\r">>, <<"\n">>]).

test() ->
    %% A one-element list behaves like its binary, with and without global.
    [<<1>>, <<3>>] = binary:split(id(<<1, 2, 3>>), [<<2>>]),
    [<<1>>, <<3, 2, 4>>] = binary:split(id(<<1, 2, 3, 2, 4>>), [<<2>>], []),
    [<<1>>, <<3>>, <<4>>] = binary:split(id(<<1, 2, 3, 2, 4>>), [<<2>>], [global]),

    %% cow_sse:parse/2 reads one line at a time with any line ending.
    [<<"data: hi">>, <<"\n">>] = binary:split(id(<<"data: hi\n\n">>), ?EOL),
    [<<>>, <<>>] = binary:split(id(<<"\n">>), ?EOL),
    [<<"event: ping">>, <<"data: x">>] =
        binary:split(id(<<"event: ping\rdata: x">>), ?EOL),
    %% CR LF is one line ending: the longest pattern at the same offset wins,
    %% whatever the order of the list.
    [<<"id: 7">>, <<"data: x\r\n">>] =
        binary:split(id(<<"id: 7\r\ndata: x\r\n">>), ?EOL),
    [<<"id: 7">>, <<"data: x\r\n">>] =
        binary:split(id(<<"id: 7\r\ndata: x\r\n">>), lists:reverse(?EOL)),
    %% A partial line has no line ending yet.
    [<<"data: partial">>] = binary:split(id(<<"data: partial">>), ?EOL),
    [<<>>] = binary:split(id(<<>>), ?EOL),

    %% cow_sse splits a field from its value on ": " or ":".
    [<<"data">>, <<"hi">>] = binary:split(id(<<"data: hi">>), [<<":\s">>, <<":">>]),
    [<<"data">>, <<"hi">>] = binary:split(id(<<"data:hi">>), [<<":\s">>, <<":">>]),
    [<<"data">>, <<" two: x">>] =
        binary:split(id(<<"data:  two: x">>), [<<":\s">>, <<":">>]),
    [<<"retry">>] = binary:split(id(<<"retry">>), [<<":\s">>, <<":">>]),
    %% The earliest match wins over a longer pattern found later.
    [<<"id">>, <<" 1\r\n">>] = binary:split(id(<<"id: 1\r\n">>), [<<"\r\n">>, <<":">>]),

    %% cow_sse:events/1 splits multi-line data on every line ending.
    [<<"one">>, <<"two">>, <<"three">>, <<"four">>] =
        binary:split(id(<<"one\r\ntwo\rthree\nfour">>), ?EOL, [global]),
    [<<"one">>, <<>>, <<"two">>, <<>>] =
        binary:split(id(<<"one\n\rtwo\r\n">>), ?EOL, [global]),
    [<<"one">>, <<"two">>] =
        binary:split(id(<<"\r\none\n\n\rtwo\r">>), ?EOL, [global, trim_all]),
    [<<"one">>, <<"two">>] =
        binary:split(id(<<"\r\none\n\n\rtwo\r">>), ?EOL, [trim_all, global]),
    [] = binary:split(id(<<"\r\n\n\r">>), ?EOL, [global, trim_all]),
    %% Global matches do not overlap: after "\r\n" the next search starts
    %% past it, so "\n\r" in the middle is never seen.
    [<<"a">>, <<>>, <<"b">>] =
        binary:split(id(<<"a\r\n\r\nb">>), [<<"\r\n">>, <<"\n\r">>], [global]),
    [<<"a">>, <<"b">>] = binary:split(id(<<"a,b">>), [<<",">>, <<",">>], [global]),

    %% A stream of 1,000 events, larger than an inline binary.
    Stream = iolist_to_binary([[<<"data: ">>, integer_to_binary(N), <<"\r\n">>]
                               || N <- lists:seq(1, 1000)]),
    Lines = binary:split(Stream, ?EOL, [global]),
    1001 = length(Lines),
    <<"data: 1">> = hd(Lines),
    <<"data: 1000">> = lists:nth(1000, Lines),
    <<>> = lists:last(Lines),
    %% Line 500 starts at byte 9 * 9 + 90 * 10 + 400 * 11 = 5381; the 200
    %% bytes there are a view into the stream's shared bytes.
    [<<"data: 500">>, <<"data: 501\r\n", Rest/binary>>] =
        binary:split(binary:part(Stream, 5381, 200), ?EOL),
    178 = byte_size(Rest),

    %% apply/3 and fun references reach the same functions.
    [<<"a">>, <<"b\nc">>] = apply(binary, split, [id(<<"a\r\nb\nc">>), ?EOL]),
    [<<"a">>, <<"b">>, <<"c">>] =
        apply(binary, split, [id(<<"a\r\nb\nc">>), ?EOL, [global]]),
    Split2 = fun binary:split/2,
    Split3 = fun binary:split/3,
    [<<"k">>, <<"v">>] = Split2(id(<<"k: v">>), [<<":\s">>, <<":">>]),
    [<<"x">>, <<"y">>] = Split3(id(<<"x\ny\n">>), ?EOL, [global, trim_all]),

    %% A pattern is a nonempty binary or a nonempty proper list of them.
    lists:foreach(fun check_badarg/1,
                  [[], [<<>>], [<<"a">>, <<>>], [<<>>, <<"a">>], [<<"a">> | <<"b">>],
                   [<<"a">>, atom], [[<<"a">>]], "a", [<<"a">>, <<1:3>>], <<1:3>>,
                   <<>>, atom, 42]),
    io:format("binary_split_patterns_ok~n"),
    ok.

check_badarg(Pattern) ->
    badarg = failure(fun() -> binary:split(id(<<"a,b">>), Pattern) end),
    badarg = failure(fun() -> binary:split(id(<<"a,b">>), Pattern, [global]) end),
    badarg = failure(fun() -> binary:split(id(<<>>), Pattern) end),
    badarg = failure(fun() -> apply(binary, split, [id(<<"a,b">>), Pattern]) end).

failure(F) ->
    try F() of
        Value -> {unexpected_return, Value}
    catch
        error:Reason -> Reason
    end.

id(X) -> X.
