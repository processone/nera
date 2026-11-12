%% EXPECT: gen_server_init_crash_ok
%% Regression: when init/1 of a gen_server or a supervisor raises, exits,
%% throws or returns a bad value, gen_server:start_link/3,4, gen_server:start/3
%% and supervisor:start_link/2,3 return {error, Reason} to a caller that traps
%% exits, leave no 'EXIT' or 'DOWN' message behind and free a registered name,
%% while a linked caller that does not trap exits exits with that Reason, as
%% on BEAM (OTP 26 and later). Before the fix, the native caller waited
%% forever for an acknowledgement the crashed process never sent, so a
%% supervisor, which traps exits, also hung starting a child whose init/1
%% crashed. Also covered, as on BEAM: init/1 returning {error, Reason}, a
%% supervisor with invalid flags, and a name that is already registered.
%% init/1 returning or throwing ignore makes the start return ignore, and the
%% process exits normally, so even a linked caller that does not trap exits
%% gets ignore; before that fix, native Nera returned {error, ignore}.
%% Run this same fixture on native Nera and BEAM.
-module(gen_server_init_crash).
-export([test/0, init/1]).

test() ->
    process_flag(trap_exit, true),

    %% gen_server, from a caller that traps exits.
    {error, {{badmatch, 2}, Stack}} =
        gen_server:start_link(?MODULE, {server, error}, []),
    true = is_list(Stack),
    no_messages(),
    {error, boom} = gen_server:start_link(?MODULE, {server, exit}, []),
    no_messages(),
    %% A thrown value is init/1's return value, as in OTP.
    {error, thrown} = gen_server:start_link(?MODULE, {server, throw}, []),
    no_messages(),
    {error, {bad_return_value, bad}} =
        gen_server:start_link(?MODULE, {server, bad_return}, []),
    no_messages(),
    {error, {{badmatch, 2}, _}} =
        gen_server:start_link({local, gen_server_init_crash_name}, ?MODULE,
                              {server, error}, []),
    undefined = whereis(gen_server_init_crash_name),
    no_messages(),
    {error, {{badmatch, 2}, _}} = gen_server:start(?MODULE, {server, error}, []),
    no_messages(),
    %% OTP 26 and later: {error, Reason} from init/1 is returned as is.
    {error, why} = gen_server:start_link({local, gen_server_init_crash_name},
                                         ?MODULE, {server, error_return}, []),
    undefined = whereis(gen_server_init_crash_name),
    no_messages(),
    %% init/1 returning or throwing ignore: the start returns ignore.
    ignore = gen_server:start_link(?MODULE, {server, ignore}, []),
    no_messages(),
    ignore = gen_server:start_link({local, gen_server_init_crash_name}, ?MODULE,
                                   {server, throw_ignore}, []),
    undefined = whereis(gen_server_init_crash_name),
    no_messages(),
    ignore = gen_server:start(?MODULE, {server, ignore}, []),
    no_messages(),

    %% A name that is already registered: init/1 is not run.
    {ok, Server} = gen_server:start({local, gen_server_init_crash_name},
                                    ?MODULE, {server, ok}, []),
    {error, {already_started, Server}} =
        gen_server:start_link({local, gen_server_init_crash_name}, ?MODULE,
                              {server, error}, []),
    no_messages(),
    ServerRef = monitor(process, Server),
    exit(Server, kill),
    receive {'DOWN', ServerRef, process, Server, killed} -> ok end,
    no_messages(),

    %% supervisor, from a caller that traps exits.
    {error, {{badmatch, 2}, SupStack}} =
        supervisor:start_link(?MODULE, {sup, error}),
    true = is_list(SupStack),
    no_messages(),
    {error, boom} = supervisor:start_link(?MODULE, {sup, exit}),
    no_messages(),
    {error, {bad_return, {?MODULE, init, bad}}} =
        supervisor:start_link(?MODULE, {sup, bad_return}),
    no_messages(),
    %% A supervisor's init/1 may not return {stop, Reason}, but OTP reads a
    %% thrown {stop, Reason} as a gen_server's init/1 return value.
    {error, {bad_return, {?MODULE, init, {stop, why}}}} =
        supervisor:start_link(?MODULE, {sup, stop}),
    no_messages(),
    {error, why} = supervisor:start_link(?MODULE, {sup, throw}),
    no_messages(),
    ignore = supervisor:start_link(?MODULE, {sup, ignore}),
    no_messages(),
    ignore = supervisor:start_link({local, supervisor_init_crash_name}, ?MODULE,
                                   {sup, throw_ignore}),
    undefined = whereis(supervisor_init_crash_name),
    no_messages(),
    {error, {{badmatch, 2}, _}} =
        supervisor:start_link({local, supervisor_init_crash_name}, ?MODULE,
                              {sup, error}),
    undefined = whereis(supervisor_init_crash_name),
    no_messages(),
    %% Invalid flags fail the start after init/1 returned. The reason differs
    %% between BEAM and Nera, so only the failure itself is checked.
    {error, _} = supervisor:start_link({local, supervisor_init_crash_name},
                                       ?MODULE, {sup, bad_flags}),
    undefined = whereis(supervisor_init_crash_name),
    no_messages(),

    %% A name that is already registered: init/1 is not run.
    {ok, Sup} = supervisor:start_link({local, supervisor_init_crash_name},
                                      ?MODULE, {sup, ok}),
    {error, {already_started, Sup}} =
        supervisor:start_link({local, supervisor_init_crash_name}, ?MODULE,
                              {sup, error}),
    no_messages(),
    exit(Sup, kill),
    receive {'EXIT', Sup, killed} -> ok end,
    no_messages(),

    %% A linked caller that does not trap exits exits with the same reason,
    %% but returns {error, Reason} when init/1 returned it.
    {exited, {{badmatch, 2}, _}} = in_plain_caller(
        fun() -> gen_server:start_link(?MODULE, {server, error}, []) end),
    {exited, {{badmatch, 2}, _}} = in_plain_caller(
        fun() -> supervisor:start_link(?MODULE, {sup, error}) end),
    {exited, thrown} = in_plain_caller(
        fun() -> gen_server:start_link(?MODULE, {server, throw}, []) end),
    {returned, {error, why}} = in_plain_caller(
        fun() -> gen_server:start_link(?MODULE, {server, error_return}, []) end),
    {returned, ignore} = in_plain_caller(
        fun() -> gen_server:start_link(?MODULE, {server, ignore}, []) end),
    {returned, ignore} = in_plain_caller(
        fun() -> supervisor:start_link(?MODULE, {sup, ignore}) end),

    %% gen_server:start/3 does not link, so it returns either way.
    process_flag(trap_exit, false),
    {error, {{badmatch, 2}, _}} = gen_server:start(?MODULE, {server, error}, []),
    no_messages(),

    io:format("gen_server_init_crash_ok~n"),
    ok.

init({server, ok}) -> {ok, state};
init({server, error}) -> 1 = id(2), {ok, state};
init({server, exit}) -> exit(boom);
init({server, throw}) -> throw({stop, thrown});
init({server, error_return}) -> {error, why};
init({server, ignore}) -> ignore;
init({server, throw_ignore}) -> throw(ignore);
init({server, bad_return}) -> bad;
init({sup, ok}) -> {ok, {{one_for_one, 1, 5}, []}};
init({sup, error}) -> 1 = id(2), ignore;
init({sup, exit}) -> exit(boom);
init({sup, bad_flags}) -> {ok, {{one_for_one}, []}};
init({sup, stop}) -> {stop, why};
init({sup, throw}) -> throw({stop, why});
init({sup, ignore}) -> ignore;
init({sup, throw_ignore}) -> throw(ignore);
init({sup, bad_return}) -> bad.

%% Run Start in a fresh process, which does not trap exits, and return
%% {returned, Result} when Start returns or {exited, Reason} when the process
%% exits first.
in_plain_caller(Start) ->
    Self = self(),
    {Pid, Ref} = spawn_monitor(fun() -> Self ! {returned, self(), Start()} end),
    receive
        {'DOWN', Ref, process, Pid, Reason} ->
            receive
                {returned, Pid, Result} -> {returned, Result}
            after 0 -> {exited, Reason}
            end
    after 5000 ->
        error(plain_caller_still_waiting)
    end.

no_messages() ->
    receive
        Msg -> error({unexpected_message, Msg})
    after 0 -> ok
    end.

id(X) -> X.
