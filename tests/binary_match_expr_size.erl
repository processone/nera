-module(binary_match_expr_size).
-export([start/0]).

start() ->
    %% Arithmetic expression in binary match size field
    <<"hello">> = strip_crlf(<<"hello\r\n">>),
    <<"hello">> = strip_crlf(<<"hello\n">>),
    <<"hello\r\n!">> = strip_crlf(<<"hello\r\n!">>),  %% no match, returned as-is
    <<"">> = strip_crlf(<<"\r\n">>),
    <<>> = strip_crlf(<<"\n">>),

    %% Variable-based size in binary match
    Sz = 3,
    <<X:Sz/binary, _/binary>> = <<"abcdef">>,
    <<"abc">> = X,

    %% Expression with variable in size field
    Bin = <<"abcdef">>,
    Len = byte_size(Bin),
    <<First:(Len - 2)/binary, _:2/binary>> = Bin,
    <<"abcd">> = First,

    ok.

strip_crlf(Bin) when is_binary(Bin) ->
    Sz = byte_size(Bin),
    case Bin of
        <<Data:(Sz - 2)/binary, "\r\n">> -> Data;
        <<Data2:(Sz - 1)/binary, "\n">> -> Data2;
        _ -> Bin
    end;
strip_crlf(Bin) -> Bin.
