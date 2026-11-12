%% Minimal proplists implementation for Nera.
%%
%% Proplists are association lists of {Key, Value} tuples (and bare atoms
%% which are shorthand for {Key, true}). The full OTP module ships ~50
%% functions; this file implements just the handful needed by current
%% Nera integration tests. Vendoring the full OTP module is blocked on
%% map-comprehension parsing support.
-module(proplists).

-export([get_value/2, get_value/3, is_defined/2, lookup/2,
         get_bool/2, get_keys/1, get_all_values/2]).

%% get_value(Key, List) — return the value for Key, or 'undefined'.
%% Bare atom Key in List counts as {Key, true}.
get_value(Key, List) ->
    get_value(Key, List, undefined).

%% get_value(Key, List, Default) — same with caller-supplied default.
get_value(_Key, [], Default) ->
    Default;
get_value(Key, [{Key, Value} | _Rest], _Default) ->
    Value;
get_value(Key, [Key | _Rest], _Default) when is_atom(Key) ->
    true;
get_value(Key, [_Other | Rest], Default) ->
    get_value(Key, Rest, Default).

%% is_defined(Key, List) — true if Key has any entry in List.
is_defined(_Key, []) ->
    false;
is_defined(Key, [{Key, _} | _]) ->
    true;
is_defined(Key, [Key | _]) when is_atom(Key) ->
    true;
is_defined(Key, [_ | Rest]) ->
    is_defined(Key, Rest).

%% lookup(Key, List) — return the first matching tuple or 'none'.
lookup(_Key, []) ->
    none;
lookup(Key, [{Key, _} = Tuple | _]) ->
    Tuple;
lookup(Key, [Key | _]) when is_atom(Key) ->
    {Key, true};
lookup(Key, [_ | Rest]) ->
    lookup(Key, Rest).

%% get_bool(Key, List) — like get_value/2 but coerces non-boolean to false.
get_bool(Key, List) ->
    case get_value(Key, List, false) of
        true -> true;
        _ -> false
    end.

%% get_keys(List) — list of unique keys present in List.
get_keys(List) ->
    get_keys(List, []).

get_keys([], Acc) ->
    lists:reverse(Acc);
get_keys([{Key, _} | Rest], Acc) ->
    case lists:member(Key, Acc) of
        true -> get_keys(Rest, Acc);
        false -> get_keys(Rest, [Key | Acc])
    end;
get_keys([Key | Rest], Acc) when is_atom(Key) ->
    case lists:member(Key, Acc) of
        true -> get_keys(Rest, Acc);
        false -> get_keys(Rest, [Key | Acc])
    end;
get_keys([_ | Rest], Acc) ->
    get_keys(Rest, Acc).

%% Return all matching values in source order, retaining duplicates.
get_all_values(_Key, []) ->
    [];
get_all_values(Key, [{Key, Value} | Rest]) ->
    [Value | get_all_values(Key, Rest)];
get_all_values(Key, [Key | Rest]) when is_atom(Key) ->
    [true | get_all_values(Key, Rest)];
get_all_values(Key, [_ | Rest]) ->
    get_all_values(Key, Rest).
