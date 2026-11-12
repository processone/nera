%% Advanced tests for reference-counted binaries.
%% Covers gaps: file I/O, comparison, format, GC stress, process death, concurrency.
%% EXPECT: <<1,2,3,0,0
-module(binary_refc_advanced).
-export([test/0]).

test() ->
    test_file_io_large(),
    test_compare_equal(),
    test_compare_different(),
    test_compare_heap_vs_large(),
    test_format_large(),
    test_gc_stress(),
    test_process_death_cleanup(),
    test_concurrent_read(),
    ok.

%% --- File I/O with large binary (exercises alloc_binary_auto in io_pool) ---
test_file_io_large() ->
    Path = "/tmp/nera_refc_test_adv.bin",
    %% Build a 200-byte binary: [1,2,3, 0*197]
    Prefix = <<1, 2, 3>>,
    Padding = <<0:1576>>,  %% 197 zero bytes
    Original = <<Prefix/binary, Padding/binary>>,
    200 = byte_size(Original),

    ok = file:write_file(Path, Original),
    {ok, ReadBack} = file:read_file(Path),

    %% The read result goes through alloc_binary_auto -> refc binary (200 > 64)
    200 = byte_size(ReadBack),
    <<1, 2, 3, _/binary>> = ReadBack,

    %% Compare file-read binary (refc) with constructed binary (heap)
    true = (Original == ReadBack),
    true = (Original =:= ReadBack),
    ok.

%% --- Compare two independently constructed identical large binaries ---
test_compare_equal() ->
    B1 = make_binary(100, 42),
    B2 = make_binary(100, 42),
    true = (B1 == B2),
    true = (B1 =:= B2),
    false = (B1 /= B2),
    ok.

%% --- Compare two different large binaries, verify ordering ---
test_compare_different() ->
    B1 = make_binary(100, 1),   %% starts with byte 1
    B2 = make_binary(100, 2),   %% starts with byte 2
    true = (B1 /= B2),
    false = (B1 == B2),
    %% B1 < B2 because first byte 1 < 2
    true = (B1 < B2),
    true = (B2 > B1),
    ok.

%% --- Compare heap binary (small) vs large binary ---
test_compare_heap_vs_large() ->
    Small = <<1>>,
    Large = make_binary(100, 1),  %% <<1, 0, 0, ...>>
    true = (Small /= Large),
    %% Small is a prefix of Large → Small < Large (lexicographic)
    true = (Small < Large),
    true = (Large > Small),
    ok.

%% --- Format a large binary that went through file I/O (refc) ---
test_format_large() ->
    Path = "/tmp/nera_refc_fmt.bin",
    %% Build 65-byte binary (just over threshold): [1,2,3, 0*62]
    Prefix = <<1, 2, 3>>,
    Padding = <<0:496>>,  %% 62 zero bytes
    Bin = <<Prefix/binary, Padding/binary>>,
    65 = byte_size(Bin),

    ok = file:write_file(Path, Bin),
    {ok, RefcBin} = file:read_file(Path),
    65 = byte_size(RefcBin),

    %% Format the refc binary — verifies format_boxed BOXED_REFC_BINARY path
    %% Output checked via %% EXPECT: above
    io:format("~w~n", [RefcBin]),
    ok.

%% --- GC stress: many allocations while holding a refc binary ---
test_gc_stress() ->
    Path = "/tmp/nera_refc_gc.bin",
    Data = make_binary(200, 99),
    ok = file:write_file(Path, Data),
    {ok, RefcBin} = file:read_file(Path),
    200 = byte_size(RefcBin),

    %% Allocate lots of short-lived data to trigger GC
    gc_pressure(1000),

    %% Verify the refc binary survived GC
    200 = byte_size(RefcBin),
    <<99, _/binary>> = RefcBin,
    ok.

gc_pressure(0) -> ok;
gc_pressure(N) ->
    %% Create a large tuple + list to burn heap space
    _ = {N, N+1, N+2, [N, N+1, N+2, N+3, N+4]},
    gc_pressure(N - 1).

%% --- Process death: Arcs should be dropped when processes die ---
test_process_death_cleanup() ->
    process_flag(trap_exit, true),
    Path = "/tmp/nera_refc_death.bin",
    Data = make_binary(200, 77),
    ok = file:write_file(Path, Data),
    {ok, RefcBin} = file:read_file(Path),

    %% Spawn 50 processes, each holding a copy of the refc binary
    spawn_holders(50, RefcBin),

    %% Wait for all 50 to exit
    drain_n_exits(50),

    %% System should be healthy — verify by doing more work
    Self = self(),
    spawn_link(fun() ->
        B = make_binary(100, 1),
        100 = byte_size(B),
        Self ! health_ok
    end),
    receive
        health_ok -> ok
    after 2000 ->
        exit(health_check_failed)
    end,

    %% Drain the health check process EXIT
    receive {'EXIT', _, normal} -> ok after 200 -> ok end,

    process_flag(trap_exit, false),
    ok.

spawn_holders(0, _Bin) -> ok;
spawn_holders(N, Bin) ->
    spawn_link(fun() ->
        %% Hold the binary briefly, then exit
        _ = byte_size(Bin),
        ok
    end),
    spawn_holders(N - 1, Bin).

drain_n_exits(0) -> ok;
drain_n_exits(N) ->
    receive
        {'EXIT', _, _} -> drain_n_exits(N - 1)
    after 2000 ->
        exit(drain_timeout)
    end.

%% --- Concurrent reads: multiple processes reading same binary ---
test_concurrent_read() ->
    B = make_binary(100, 1),
    Self = self(),
    N = 10,
    spawn_readers(N, B, Self),
    collect_oks(N),
    ok.

spawn_readers(0, _B, _Parent) -> ok;
spawn_readers(N, B, Parent) ->
    spawn(fun() ->
        %% Pattern match to read the binary
        <<1, _/binary>> = B,
        100 = byte_size(B),
        Parent ! reader_ok
    end),
    spawn_readers(N - 1, B, Parent).

collect_oks(0) -> ok;
collect_oks(N) ->
    receive
        reader_ok -> collect_oks(N - 1)
    after 2000 ->
        exit(concurrent_read_timeout)
    end.

%% Helper: make an N-byte binary where the first byte is Val, rest are zeros.
make_binary(Size, Val) ->
    Rest = Size - 1,
    RestBits = Rest * 8,
    <<Val, 0:RestBits>>.
