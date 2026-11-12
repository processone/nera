%% Minimal timer module for nera.
%%
%% Implements the most commonly used functions from OTP's timer module.
%% Uses erlang:send_after/3 and receive...after for scheduling.

-module(timer).
-export([sleep/1,
         send_after/2, send_after/3,
         apply_after/4,
         tc/1, tc/2, tc/3,
         seconds/1, minutes/1, hours/1,
         hms/3]).

%% sleep(Time) -> ok
%% Suspend the calling process for Time milliseconds.
sleep(Time) when is_integer(Time), Time >= 0 ->
    receive after Time -> ok end;
sleep(infinity) ->
    receive after infinity -> ok end.

%% send_after(Time, Message) -> {ok, TRef}
%% Send Message to the calling process after Time milliseconds.
send_after(Time, Message) ->
    send_after(Time, self(), Message).

%% send_after(Time, Pid, Message) -> {ok, TRef}
%% Send Message to Pid after Time milliseconds.
send_after(Time, Pid, Message) when is_integer(Time), Time >= 0 ->
    TRef = erlang:send_after(Time, Pid, Message),
    {ok, TRef}.

%% apply_after(Time, Module, Function, Arguments) -> {ok, TRef}
%% Evaluate apply(Module, Function, Arguments) after Time milliseconds.
apply_after(Time, Module, Function, Arguments) when is_integer(Time), Time >= 0 ->
    Pid = spawn(fun() ->
        receive
            {apply, M, F, A} ->
                apply(M, F, A)
        end
    end),
    TRef = erlang:send_after(Time, Pid, {apply, Module, Function, Arguments}),
    {ok, TRef}.

%% tc(Fun) -> {Time, Value}
%% Measure the execution time of Fun in microseconds.
tc(Fun) ->
    T0 = erlang:monotonic_time(microsecond),
    Value = Fun(),
    T1 = erlang:monotonic_time(microsecond),
    {T1 - T0, Value}.

%% tc(Fun, Arguments) -> {Time, Value}
%% Measure apply(Fun, Arguments) in microseconds.
tc(Fun, Arguments) ->
    T0 = erlang:monotonic_time(microsecond),
    Value = apply(Fun, Arguments),
    T1 = erlang:monotonic_time(microsecond),
    {T1 - T0, Value}.

%% tc(Module, Function, Arguments) -> {Time, Value}
%% Measure apply(Module, Function, Arguments) in microseconds.
tc(Module, Function, Arguments) ->
    T0 = erlang:monotonic_time(microsecond),
    Value = apply(Module, Function, Arguments),
    T1 = erlang:monotonic_time(microsecond),
    {T1 - T0, Value}.

%% Time conversion utilities.

%% seconds(Seconds) -> Milliseconds
seconds(Seconds) ->
    1000 * Seconds.

%% minutes(Minutes) -> Milliseconds
minutes(Minutes) ->
    1000 * 60 * Minutes.

%% hours(Hours) -> Milliseconds
hours(Hours) ->
    1000 * 60 * 60 * Hours.

%% hms(Hours, Minutes, Seconds) -> Milliseconds
hms(Hours, Minutes, Seconds) ->
    hours(Hours) + minutes(Minutes) + seconds(Seconds).
