%% Tests basic gen_server functionality.
-module(gen_server_basic).
-export([test/0]).
-export([init/1, handle_call/3, handle_cast/2, handle_info/2, terminate/2]).

test() ->
    %% Start a gen_server
    {ok, Pid} = gen_server:start(?MODULE, 0, []),
    true = is_process_alive(Pid),

    %% Synchronous call - get value
    0 = gen_server:call(Pid, get),

    %% Synchronous call - increment
    1 = gen_server:call(Pid, increment),
    1 = gen_server:call(Pid, get),

    %% Increment again
    2 = gen_server:call(Pid, increment),
    2 = gen_server:call(Pid, get),

    %% Async cast - reset
    ok = gen_server:cast(Pid, reset),
    receive after 10 -> ok end,
    0 = gen_server:call(Pid, get),

    %% Stop the server
    ok = gen_server:cast(Pid, stop),
    receive after 50 -> ok end,
    false = is_process_alive(Pid),

    io:format("ok~n"),
    ok.

%% gen_server callbacks
init(InitialValue) ->
    {ok, InitialValue}.

handle_call(get, _From, State) ->
    {reply, State, State};
handle_call(increment, _From, State) ->
    NewState = State + 1,
    {reply, NewState, NewState};
handle_call(_Request, _From, State) ->
    {reply, ok, State}.

handle_cast(reset, _State) ->
    {noreply, 0};
handle_cast(stop, State) ->
    {stop, normal, State};
handle_cast(_Msg, State) ->
    {noreply, State}.

handle_info(_Info, State) ->
    {noreply, State}.

terminate(_Reason, _State) ->
    ok.
