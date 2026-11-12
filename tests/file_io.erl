-module(main).
-export([start/0]).

start() ->
    test(),
    ok.

test() ->
    %% 1. write_file + read_file round-trip with binary
    Filename = "/tmp/nera_test_file_io.txt",
    Content = <<"Hello, nera!">>,
    ok = file:write_file(Filename, Content),
    {ok, ReadBack} = file:read_file(Filename),
    Content = ReadBack,  %% badmatch if different

    %% 2. read_file on nonexistent file returns {error, enoent}
    {error, enoent} = file:read_file("/tmp/nera_nonexistent_file_12345.txt"),

    %% 3. write_file to invalid path returns {error, enoent}
    {error, enoent} = file:write_file("/tmp/no_such_dir_999/test.txt", <<"data">>),

    %% 4. write_file with string filename (list of char codes)
    Filename2 = "/tmp/nera_test_file_io2.txt",
    ok = file:write_file(Filename2, <<"string path test">>),
    {ok, <<"string path test">>} = file:read_file(Filename2),

    %% 5. write_file with iolist data (list of byte values)
    Filename3 = "/tmp/nera_test_file_io3.txt",
    ok = file:write_file(Filename3, [72, 105]),  %% "Hi"
    {ok, <<"Hi">>} = file:read_file(Filename3),

    ok.
