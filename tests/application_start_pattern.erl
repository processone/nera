%% MODULES: _myapp_app.erl _myapp_sup.erl _myapp_worker.erl
-module(application_start_pattern).
-export([test/0]).

%% Test the standard OTP application:start pattern:
%%   application:start(sasl),
%%   application:start(myapp)
%%
%% This validates the full chain:
%%   application:start(myapp)
%%     -> myapp_app:start(normal, [])     (dynamic call via app_module/1)
%%       -> myapp_sup:start_link()        (supervisor)
%%         -> myapp_worker:start_link()   (gen_server child)

test() ->
    %% Step 1: start sasl (no-op in nera)
    ok = application:start(sasl),

    %% Step 2: start the application via standard OTP pattern
    ok = application:start(myapp),

    %% Step 3: verify supervisor is registered and running
    SupPid = whereis(myapp_sup),
    true = is_pid(SupPid),
    true = is_process_alive(SupPid),

    %% Step 4: verify worker is registered and running
    WorkerPid = whereis(myapp_worker),
    true = is_pid(WorkerPid),
    true = is_process_alive(WorkerPid),

    %% Step 5: verify worker responds to gen_server calls
    0 = gen_server:call(myapp_worker, get_count),
    1 = gen_server:call(myapp_worker, increment),
    1 = gen_server:call(myapp_worker, get_count),

    %% Step 6: stop the application
    ok = application:stop(myapp),

    %% Clean exit
    halt(0).
