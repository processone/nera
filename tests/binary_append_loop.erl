%% Tests: building binaries incrementally in a loop, various sizes.
-module(binary_append_loop).
-export([test/0]).

test() ->
    %% Build binary byte-by-byte
    B100 = build(100),
    100 = byte_size(B100),
    ok = verify(B100, 0),

    B1000 = build(1000),
    1000 = byte_size(B1000),
    ok = verify(B1000, 0),

    %% Build with larger chunks
    B500 = build_chunks(<<>>, 0, 100, 5),
    500 = byte_size(B500),

    %% Append in comprehension-like loop
    B200 = append_list(lists:seq(0, 199)),
    200 = byte_size(B200),

    %% Round-trip: build then split
    Original = lists:seq(0, 255),
    Bin = append_list(Original),
    256 = byte_size(Bin),
    Original = binary_to_intlist(Bin),

    io:format("ok~n"),
    ok.

%% Build binary one byte at a time
build(N) ->
    build(N, 0, <<>>).

build(N, N, Acc) ->
    Acc;
build(N, I, Acc) ->
    Byte = I rem 256,
    build(N, I + 1, <<Acc/binary, Byte>>).

%% Verify each byte matches I rem 256
verify(<<>>, _) ->
    ok;
verify(<<B, Rest/binary>>, I) ->
    B = I rem 256,
    verify(Rest, I + 1).

%% Build by appending 5-byte chunks
build_chunks(Acc, Count, Count, _ChunkSize) ->
    Acc;
build_chunks(Acc, I, Count, ChunkSize) ->
    Chunk = make_chunk(ChunkSize, I),
    build_chunks(<<Acc/binary, Chunk/binary>>, I + 1, Count, ChunkSize).

make_chunk(Size, Seed) ->
    make_chunk(Size, Seed, <<>>).

make_chunk(0, _, Acc) -> Acc;
make_chunk(N, Seed, Acc) ->
    B = (Seed + N) rem 256,
    make_chunk(N - 1, Seed, <<Acc/binary, B>>).

%% Build binary from list of integers
append_list(List) ->
    append_list(List, <<>>).

append_list([], Acc) -> Acc;
append_list([H|T], Acc) ->
    append_list(T, <<Acc/binary, H>>).

%% Convert binary back to list of integers
binary_to_intlist(<<>>) -> [];
binary_to_intlist(<<B, Rest/binary>>) -> [B | binary_to_intlist(Rest)].
