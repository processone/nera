-module(binary_part).
-export([test/0]).

id(I) -> I.

test() ->
    %% Basic binary_part/3
    Bin = id(<<"hello world">>),
    <<"hello">> = binary_part(Bin, 0, 5),
    <<"world">> = binary_part(Bin, 6, 5),
    <<"lo wo">> = binary_part(Bin, 3, 5),

    %% Single byte
    <<"h">> = binary_part(Bin, 0, 1),

    %% Full binary
    <<"hello world">> = binary_part(Bin, 0, 11),

    %% Empty result
    <<>> = binary_part(Bin, 0, 0),

    %% Reproducer from Bug #6: command line stripping \r\n
    Cmd = id(<<"CMD 20\r\n">>),
    <<"CMD 20">> = binary_part(Cmd, 0, byte_size(Cmd) - 2),

    %% Negative length (count backwards from Pos)
    <<"world">> = binary_part(Bin, 11, -5),

    ok.
