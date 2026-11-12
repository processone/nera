%% EXPECT: import_resolution_ok
%% NERA-92: accepted OTP module-resolution policy, including optimized call names.
-module(import_resolution).
-export([test/0]).
-compile([debug_info, {no_auto_import, [self/0, spawn/3, byte_size/1, bit_size/1, throw/1]}]).
-import(lists, [reverse/1]).

test() ->
    local_self = self(),
    local_spawn = spawn(?MODULE, test, []),
    local_throw = throw(ignored),
    Self = fun self/0,
    local_self = Self(),
    Throw = fun throw/1,
    local_throw = Throw(ignored),
    {local_bytes, local_bits} = sizes(<<1,2,3>>),
    [3,2,1] = reverse([1,2,3]),
    3 = length([1,2,3]),
    Length = fun length/1,
    2 = Length([a,b]),
    true = is_pid(erlang:self()),
    io:format("import_resolution_ok~n"),
    ok.

self() -> local_self.
spawn(_, _, _) -> local_spawn.
throw(_) -> local_throw.
byte_size(_) -> local_bytes.
bit_size(_) -> local_bits.
sizes(<<_:8, Rest/bits>>) -> {byte_size(Rest), bit_size(Rest)}.
