%% Tests error_logger basic functionality.
%% We just verify it doesn't crash - output goes to stderr.
-module(error_logger_basic).
-export([test/0]).

test() ->
    %% info_msg
    error_logger:info_msg("Server started on port ~p~n", [8080]),

    %% warning_msg
    error_logger:warning_msg("Slow query: ~p ms~n", [250]),

    %% error_msg
    error_logger:error_msg("Connection failed: ~p~n", [timeout]),

    %% With binary data
    error_logger:info_msg("Key: ~p~n", [<<"session_abc">>]),

    %% With tuple data
    error_logger:error_msg("Error: ~p~n", [{badmatch, {error, timeout}}]),

    %% With string args
    error_logger:info_msg("Host ~s port ~p~n", ["127.0.0.1", 11211]),

    io:format("ok~n"),
    ok.
