%% Experimental binary fast path; the shipped Erlang module remains the oracle.
-module(base64_rust).
-export([encode/2, decode/2]).
encode(Data, Options) when is_binary(Data), is_map(Options) ->
    nera_base64_native:encode(Data, maps:get(mode, Options, standard) =:= urlsafe,
                            maps:get(padding, Options, true) =/= false);
encode(Data, Options) -> base64:encode(Data, Options).
decode(Data, Options) when is_binary(Data), is_map(Options) ->
    case nera_base64_native:decode(Data, maps:get(mode, Options, standard) =:= urlsafe,
                                 maps:get(padding, Options, true) =/= false) of
        fallback -> base64:decode(Data, Options);
        Binary -> Binary
    end;
decode(Data, Options) -> base64:decode(Data, Options).
