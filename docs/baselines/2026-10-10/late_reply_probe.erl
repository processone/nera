%% Diagnostic, not a passing conformance test: modern OTP prints no_late_reply.
%% Compile and run on both Nera and BEAM; see README.md in this directory.
-module(late_reply_probe).
-export([start/0]).

start() ->
    Parent = self(),
    Server = spawn(fun() ->
        receive
            {'$gen_call', From, ping} ->
                %% Reply only after the caller has caught the timeout.
                receive
                    release ->
                        gen_server:reply(From, pong),
                        Parent ! replied
                end
        end
    end),
    {'EXIT', _} = (catch gen_server:call(Server, ping, 10)),
    Server ! release,
    receive
        replied -> ok
    after 2000 ->
        erlang:error(server_did_not_reply)
    end,
    receive
        {_, pong} -> io:format("late_reply~n")
    after 0 ->
        io:format("no_late_reply~n")
    end.
