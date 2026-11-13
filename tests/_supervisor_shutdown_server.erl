%% Helper for supervisor_shutdown: a gen_server that traps exits and does not
%% export terminate/2, which OTP makes optional.
-module(supervisor_shutdown_server).
-export([start_link/2]).
-export([init/1, handle_call/3, handle_cast/2, handle_info/2]).

start_link(Id, Test) ->
    gen_server:start_link(?MODULE, {Id, Test}, []).

init({Id, Test}) ->
    process_flag(trap_exit, true),
    Test ! {started, Id, self()},
    {ok, Id}.

handle_call(_Request, _From, State) -> {reply, ok, State}.

handle_cast(_Msg, State) -> {noreply, State}.

handle_info(_Info, State) -> {noreply, State}.
