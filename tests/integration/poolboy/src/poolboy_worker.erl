%% Minimal gen_server worker used by the poolboy integration test.
%% Replies to ping with pong, echoes terms back, and ignores casts/info.
-module(poolboy_worker).
-behaviour(gen_server).

-export([start_link/1]).
-export([init/1, handle_call/3, handle_cast/2, handle_info/2,
         terminate/2, code_change/3]).

start_link(Args) ->
    gen_server:start_link(?MODULE, Args, []).

init(_Args) ->
    {ok, #{}}.

handle_call(ping, _From, State) ->
    {reply, pong, State};
handle_call({echo, X}, _From, State) ->
    {reply, X, State};
handle_call(_Req, _From, State) ->
    {reply, ok, State}.

handle_cast(_Msg, State) ->
    {noreply, State}.

handle_info(_Info, State) ->
    {noreply, State}.

terminate(_Reason, _State) ->
    ok.

code_change(_OldVsn, State, _Extra) ->
    {ok, State}.
