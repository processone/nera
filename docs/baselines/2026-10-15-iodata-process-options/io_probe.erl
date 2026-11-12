-module(io_probe).
-export([main/0]).
main() ->
    Payload = binary:copy(<<42>>, 1024), Flat = binary_to_list(Payload),
    Nested = [lists:duplicate(512, 42), [binary:copy(<<42>>, 512)]],
    {ok, L} = gen_tcp:listen(0, [binary, {active, false}]),
    {ok, Port} = inet:port(L),
    {ok, C} = gen_tcp:connect("127.0.0.1", Port, [binary, {active, false}]),
    {ok, S} = gen_tcp:accept(L),
    bench_tcp(tcp_binary, C, S, Payload, Payload),
    bench_tcp(tcp_flat, C, S, Flat, Payload),
    bench_tcp(tcp_nested, C, S, Nested, Payload),
    gen_tcp:close(C), gen_tcp:close(S), gen_tcp:close(L),
    bench_file(file_binary, Payload, Payload), bench_file(file_flat, Flat, Payload).
bench_tcp(Name, C, S, Data, Payload) ->
    tcp_loop(C, S, Data, Payload, 16),
    {Time, ok} = timer:tc(fun() -> tcp_loop(C, S, Data, Payload, 2000) end),
    io:format("~p ~p~n", [Name, Time]).
tcp_loop(_, _, _, _, 0) -> ok;
tcp_loop(C, S, Data, Payload, N) ->
    ok = gen_tcp:send(C, Data), {ok, Payload} = gen_tcp:recv(S, 1024),
    tcp_loop(C, S, Data, Payload, N - 1).
bench_file(Name, Data, Payload) ->
    Path = "/tmp/nera_iodata_perf_test.txt",
    file_loop(Path, Data, 8),
    {Time, ok} = timer:tc(fun() -> file_loop(Path, Data, 200) end),
    {ok, Payload} = file:read_file(Path), io:format("~p ~p~n", [Name, Time]).
file_loop(_, _, 0) -> ok;
file_loop(Path, Data, N) -> ok = file:write_file(Path, Data), file_loop(Path, Data, N - 1).
