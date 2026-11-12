%% Tests for MessageFragment fast paths: InlineTuple, SimpleTuple,
%% batch allocation, and duplicate refc binary handling.
%% Exercises the message passing optimizations added in March 2026.
-module(message_fragment_paths).
-export([test/0]).

test() ->
    test_inline_tuple_immediates(),
    test_inline_tuple_refc_binary(),
    test_simple_tuple_refc_binaries(),
    test_duplicate_refc_in_tuple(),
    test_high_volume_inline_tuple(),
    test_mixed_arity_messages(),
    test_nested_tuple_falls_to_heap(),
    ok.

%% {ok, 42} — arity 2, all immediates → InlineTuple path
test_inline_tuple_immediates() ->
    Self = self(),
    spawn(fun() -> Self ! {ok, 42} end),
    receive
        {ok, 42} -> ok
    after 1000 -> exit(timeout_inline_imm)
    end.

%% {msg, <<large binary>>} — arity 2, one refc binary → InlineTuple path
test_inline_tuple_refc_binary() ->
    Bin = <<0:8000>>,  %% 1000 bytes, triggers refc (> 64)
    1000 = byte_size(Bin),
    Self = self(),
    spawn(fun() -> Self ! {msg, Bin} end),
    receive
        {msg, RecvBin} ->
            1000 = byte_size(RecvBin),
            ok
    after 1000 -> exit(timeout_inline_refc)
    end.

%% {a, b, <<large>>} — arity 3 → SimpleTuple path (MAX_INLINE_ELEMS=2)
test_simple_tuple_refc_binaries() ->
    Bin1 = <<1:8, 0:7992>>,   %% 1000 bytes, first byte = 1
    Bin2 = <<2:8, 0:7992>>,   %% 1000 bytes, first byte = 2
    Self = self(),
    spawn(fun() -> Self ! {data, Bin1, Bin2} end),
    receive
        {data, R1, R2} ->
            1000 = byte_size(R1),
            1000 = byte_size(R2),
            %% Verify content preserved across process boundary
            <<1, _/binary>> = R1,
            <<2, _/binary>> = R2,
            ok
    after 1000 -> exit(timeout_simple_tuple)
    end.

%% {Bin, Bin} — same refc binary twice in one tuple
test_duplicate_refc_in_tuple() ->
    Bin = <<0:16000>>,  %% 2000 bytes
    Self = self(),
    spawn(fun() -> Self ! {Bin, Bin} end),
    receive
        {B1, B2} ->
            2000 = byte_size(B1),
            2000 = byte_size(B2),
            ok
    after 1000 -> exit(timeout_dup_refc)
    end.

%% Send 5000 {msg, Bin} tuples — exercises InlineTuple under load,
%% verifies no Arc/GC corruption. Inspired by Joe Armstrong's
%% message-passing stress patterns.
test_high_volume_inline_tuple() ->
    Bin = <<0:8000>>,
    N = 5000,
    Self = self(),
    Pid = spawn(fun() -> volume_receiver(Self, N) end),
    volume_sender(Pid, Bin, N),
    receive
        {done, N} -> ok
    after 10000 -> exit(timeout_high_volume)
    end.

volume_sender(_Pid, _Bin, 0) -> ok;
volume_sender(Pid, Bin, N) ->
    Pid ! {msg, Bin},
    volume_sender(Pid, Bin, N - 1).

volume_receiver(Parent, 0) -> Parent ! {done, 5000};
volume_receiver(Parent, N) ->
    receive
        {msg, Bin} ->
            1000 = byte_size(Bin),
            volume_receiver(Parent, N - 1)
    end.

%% Send messages of varying arities to exercise both InlineTuple and
%% SimpleTuple paths in the same test. Mimics real programs that send
%% different message shapes (OTP gen_server {'$gen_call', ...}, etc.)
test_mixed_arity_messages() ->
    Bin = <<0:8000>>,
    Self = self(),
    spawn(fun() ->
        Self ! {one, Bin},          %% arity 2 → InlineTuple
        Self ! {two, Bin, 42},      %% arity 3 → SimpleTuple
        Self ! {three, Bin, Bin, Bin}, %% arity 4 → SimpleTuple
        Self ! done
    end),
    receive {one, B1} -> 1000 = byte_size(B1) end,
    receive {two, B2, 42} -> 1000 = byte_size(B2) end,
    receive {three, B3, B4, B5} ->
        1000 = byte_size(B3),
        1000 = byte_size(B4),
        1000 = byte_size(B5)
    end,
    receive done -> ok
    after 2000 -> exit(timeout_mixed)
    end.

%% Tuple containing a list → falls through to Heap(Box<HeapFragment>) path
test_nested_tuple_falls_to_heap() ->
    Self = self(),
    spawn(fun() -> Self ! {result, [1, 2, 3]} end),
    receive
        {result, [1, 2, 3]} -> ok
    after 1000 -> exit(timeout_heap_fallback)
    end.
