-module(apply_export_all).
-compile([export_all]).
-export([callback/0]).

%% export_all also exports the definitions absent from the explicit list.
callback() -> ready.
callback(Value) -> Value.
tail(Value) -> ?MODULE:callback(Value).
