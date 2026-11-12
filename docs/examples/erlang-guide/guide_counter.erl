-module(guide_counter).
-behaviour(gen_server).
-export([start/0, init/1, handle_call/3, handle_cast/2,
         handle_info/2, terminate/2, code_change/3]).

start() ->
    process_flag(trap_exit, true),
    {ok, Server} = gen_server:start_link(?MODULE, 0, []),
    1 = gen_server:call(Server, increment),
    2 = gen_server:call(Server, increment),
    ok = gen_server:cast(Server, reset),
    0 = gen_server:call(Server, get),
    io:format("Counter: 1, 2, then reset to 0~n"),
    ok = gen_server:call(Server, stop),
    receive
        {'EXIT', Server, normal} ->
            io:format("Counter stopped normally~n")
    after 5000 ->
        error(stop_timeout)
    end.

init(Initial) -> {ok, Initial}.

handle_call(increment, _From, Count) -> {reply, Count + 1, Count + 1};
handle_call(get, _From, Count) -> {reply, Count, Count};
handle_call(stop, _From, Count) -> {stop, normal, ok, Count}.

handle_cast(reset, _Count) -> {noreply, 0}.
handle_info(_Message, Count) -> {noreply, Count}.
terminate(_Reason, _Count) -> ok.
code_change(_OldVersion, Count, _Extra) -> {ok, Count}.
