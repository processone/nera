%% EXPECT: ok
-module(binary_referenced_byte_size).
-export([test/0]).

test() ->
    Parent = binary:copy(<<42>>, 256),
    256 = binary:referenced_byte_size(Parent),
    View = binary:part(Parent, 10, 101),
    101 = byte_size(View),
    256 = binary:referenced_byte_size(View),
    Nested = binary:part(View, 5, 65),
    256 = binary:referenced_byte_size(Nested),
    65 = binary:referenced_byte_size(binary:copy(Nested)),
    64 = binary:referenced_byte_size(binary:part(Parent, 10, 64)),
    0 = binary:referenced_byte_size(binary:part(Parent, 256, 0)),
    256 = binary:referenced_byte_size(binary:part(Parent, 111, -101)),
    F = fun binary:referenced_byte_size/1,
    256 = F(View),
    256 = apply(binary, referenced_byte_size, [View]),
    0 = binary:referenced_byte_size(<<>>),
    0 = binary:referenced_byte_size(<<1:1>>),
    1 = binary:referenced_byte_size(<<1:9>>),
    badarg = failure(fun() -> binary:referenced_byte_size(no_binary) end),
    badarg = failure(fun() -> binary:referenced_byte_size(123) end),
    badarg = failure(fun() -> binary:referenced_byte_size([]) end),
    badarg = failure(fun() -> binary:part(Parent, 1, -2) end),
    badarg = failure(fun() -> binary:part(Parent, 256, 1) end),
    io:format("ok~n").

failure(F) -> try F() of _ -> missing_error catch error:badarg -> badarg end.
