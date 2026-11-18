-module(apply_export_all_only).
-compile(export_all).

%% No -export list: export_all alone exports every definition.
start_link(Arg) -> {ok, Arg}.
reply(To, Value) -> To ! {?MODULE, Value}, ok.
