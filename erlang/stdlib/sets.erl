%% Set API subset used by EUnit, represented by maps with exact keys.
-module(sets).
-export([new/0, from_list/1, to_list/1, size/1, is_element/2,
         add_element/2, del_element/2, union/2, fold/3]).
new() -> #{}.
from_list(Elements) -> maps:from_keys(Elements, []).
to_list(Set) -> maps:keys(Set).
size(Set) -> maps:size(Set).
is_element(Element, Set) -> maps:is_key(Element, Set).
add_element(Element, Set) -> maps:put(Element, [], Set).
del_element(Element, Set) -> maps:remove(Element, Set).
union(Left, Right) -> maps:merge(Left, Right).
fold(Fun, Acc, Set) -> maps:fold(fun(Key, _, A) -> Fun(Key, A) end, Acc, Set).
