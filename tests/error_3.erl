%% EXPECT: error_3_ok
%% Intent: error(Reason, Args, Options) raises error:Reason as on OTP 29,
%% including error_info options used by OTP libraries. Run the same fixture
%% natively and on BEAM; stacktrace arguments and error_info are not compared.
-module(error_3).
-export([test/0]).

test() ->
    Mod = id(erlang),
    Name = id(error),
    Fun = fun erlang:error/3,
    Options = [{error_info, #{module => erl_stdlib_errors, cause => #{1 => bad_type}}}],
    Calls = [
        {auto_import, fun(R, A, O) -> error(R, A, O) end},
        {qualified, fun(R, A, O) -> erlang:error(R, A, O) end},
        {tail_call, fun raise_in_tail/3},
        {apply, fun(R, A, O) -> apply(erlang, error, [R, A, O]) end},
        {apply_computed, fun(R, A, O) -> apply(Mod, Name, [R, A, O]) end},
        {remote_fun, Fun},
        {apply_fun, fun(R, A, O) -> apply(Fun, [R, A, O]) end},
        {computed_module, fun(R, A, O) -> Mod:error(R, A, O) end},
        {computed_function, fun(R, A, O) -> erlang:Name(R, A, O) end}
    ],
    %% Both a small reason and a heap-allocated reason survive every path.
    Reasons = [badarg, {invalid_option, id([a, b])}],
    [begin
        {Call, Reason, {error, Reason}} =
            {Call, Reason, outcome(fun() -> F(Reason, [a], Options) end)}
    end || {Call, F} <- Calls, Reason <- Reasons],

    %% OTP 29 accepts arbitrary Args and Options without changing the reason.
    %% In particular, an unrecognized or malformed option does not raise badarg.
    Inputs = [
        {[], []},
        {none, [{error_info, #{}}]},
        {not_a_list, Options},
        {[a], not_a_list},
        {[a], [unknown_option]},
        {[a], [{error_info, not_a_map}]},
        {[a], [{error_info, #{}} | improper_tail]}
    ],
    [begin
        {Call, Args, Opts, {error, x}} =
            {Call, Args, Opts, outcome(fun() -> F(x, Args, Opts) end)}
    end || {Call, F} <- Calls, {Args, Opts} <- Inputs],

    %% Literal auto-imported calls must compile too (not just variable operands).
    {error, x} = outcome(fun() -> error(x, [a], [{error_info, #{}}]) end),

    %% A non-tail call raises before the next expression; its after body runs.
    {error, x} = outcome(fun() ->
        try
            erlang:error(x, [a], Options),
            self() ! after_error
        after
            self() ! cleanup
        end
    end),
    [cleanup] = drain(),

    %% All arguments are evaluated before raising, in source order.
    {error, x} = outcome(fun() ->
        error(mark(reason, x), mark(args, [a]), mark(options, Options))
    end),
    [reason, args, options] = drain(),
    {throw, option_failure} = outcome(fun() -> error(x, [a], throw(option_failure)) end),

    %% Ordinary catch still returns the reason and a stacktrace list.
    {'EXIT', {x, Stack}} = (catch error(x, [a], Options)),
    true = is_list(Stack),
    io:format("error_3_ok~n"),
    ok.

raise_in_tail(Reason, Args, Options) -> erlang:error(Reason, Args, Options).

outcome(F) ->
    try {ok, F()} catch Class:Reason -> {Class, Reason} end.

mark(Name, Value) -> self() ! Name, Value.

drain() -> receive M -> [M | drain()] after 0 -> [] end.

id(X) -> X.
