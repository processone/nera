%% Dictionary API subset used by EUnit. Keys use exact equality.
%% The representation is opaque and belongs to Nera.
-module(dict).
-export([new/0, size/1, find/2, store/3, erase/2, from_list/1, to_list/1]).
new() -> #{}.
size(D) -> maps:size(D).
find(Key, D) -> maps:find(Key, D).
store(Key, Value, D) -> maps:put(Key, Value, D).
erase(Key, D) -> maps:remove(Key, D).
from_list(Pairs) -> maps:from_list(Pairs).
to_list(D) -> maps:to_list(D).
