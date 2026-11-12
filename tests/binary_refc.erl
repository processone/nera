%% Tests for reference-counted (refc) binaries.
%% Binaries > 64 bytes use Arc<Vec<u8>> backing; smaller ones stay on-heap.
-module(binary_refc).
-export([test/0]).

test() ->
    test_large_binary_construct(),
    test_large_binary_pattern_match(),
    test_send_large_binary(),
    test_sub_binary_rest(),
    test_multiple_receivers(),
    test_binary_concat_large(),
    test_byte_size_large(),
    test_send_and_resend(),
    ok.

%% Construct a binary > 64 bytes and verify size
test_large_binary_construct() ->
    %% 100 zero bytes (800 bits)
    B = <<0:800>>,
    100 = byte_size(B),
    %% First byte is 0
    <<0, _/binary>> = B,
    ok.

%% Pattern match on a large binary built via concatenation
test_large_binary_pattern_match() ->
    B = make_large_binary(),
    100 = byte_size(B),
    <<1, 2, 3, Rest/binary>> = B,
    97 = byte_size(Rest),
    ok.

%% Send a large binary to another process and verify content is intact
test_send_large_binary() ->
    B = make_large_binary(),
    Self = self(),
    spawn(fun() ->
        %% Verify in the spawned process
        100 = byte_size(B),
        <<1, 2, 3, _/binary>> = B,
        Self ! {result, ok}
    end),
    receive
        {result, ok} -> ok
    after 2000 ->
        exit(timeout_send_large)
    end.

%% Extract rest sub-binary (> 64 bytes) from a large binary
test_sub_binary_rest() ->
    B = make_large_binary(),
    %% Skip first byte, rest is 99 bytes (> threshold)
    <<_, Sub/binary>> = B,
    99 = byte_size(Sub),
    %% First byte of sub should be 2 (second byte of original)
    <<2, 3, _/binary>> = Sub,
    ok.

%% Send same large binary to multiple processes
test_multiple_receivers() ->
    B = make_large_binary(),
    Self = self(),
    spawn(fun() -> Self ! {p1, byte_size(B)} end),
    spawn(fun() -> Self ! {p2, byte_size(B)} end),
    spawn(fun() -> Self ! {p3, byte_size(B)} end),
    ok = collect_responses(3),
    ok.

collect_responses(0) -> ok;
collect_responses(N) ->
    receive
        {_, 100} -> collect_responses(N - 1)
    after 2000 ->
        exit(timeout_multi_recv)
    end.

%% Concatenating binaries to create result at threshold boundary
test_binary_concat_large() ->
    A = <<0:256>>,  %% 32 bytes
    B = <<1:256>>,  %% 32 bytes
    C = <<A/binary, B/binary>>,  %% 64 bytes
    64 = byte_size(C),
    <<0, _/binary>> = C,
    ok.

%% byte_size works for large binaries
test_byte_size_large() ->
    B1 = <<0:520>>,  %% 65 bytes (just over threshold)
    65 = byte_size(B1),
    B2 = <<0:800>>,  %% 100 bytes
    100 = byte_size(B2),
    B3 = <<0:8000>>,  %% 1000 bytes
    1000 = byte_size(B3),
    ok.

%% Send binary from process A to B, then B sends it to C
test_send_and_resend() ->
    B = make_large_binary(),
    Self = self(),
    spawn(fun() ->
        %% Receive from main, verify, and forward to another process
        Pid2 = spawn(fun() ->
            receive
                {check, Bin} ->
                    100 = byte_size(Bin),
                    <<1, _/binary>> = Bin,
                    Self ! {final, ok}
            after 2000 ->
                Self ! {final, timeout}
            end
        end),
        Pid2 ! {check, B}
    end),
    receive
        {final, ok} -> ok
    after 3000 ->
        exit(timeout_resend)
    end.

%% Helper: builds a 100-byte binary with known content
%% Bytes: 1, 2, 3, 0, 0, ..., 0 (100 bytes total)
make_large_binary() ->
    Prefix = <<1, 2, 3>>,
    Padding = <<0:776>>,  %% 97 zero bytes
    <<Prefix/binary, Padding/binary>>.
