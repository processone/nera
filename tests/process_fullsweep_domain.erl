-module(process_fullsweep_domain).
-export([test/0]).
test() ->
    Old = process_flag(fullsweep_after, 7),
    {'EXIT', {badarg, _}} = (catch process_flag(fullsweep_after, 4294967296)),
    7 = process_flag(fullsweep_after, Old), ok.
