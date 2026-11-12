%% ETS values carry an external-storage bit. Inline type guards must remove
%% it before reading the boxed header, including on x86_64 without ARM TBI.
-module(ets_type_guards).
-export([test/0]).

test() ->
    T = ets:new(type_guards, [set]),
    true = ets:insert(T, {key, {nested, ok}, 3.5, #{a => 1}, <<"data">>, make_ref()}),
    [{key, Tuple, Float, Map, Binary, Ref}] = ets:lookup(T, key),
    tuple = classify(Tuple),
    float = classify(Float),
    map = classify(Map),
    bitstring = classify(Binary),
    reference = classify(Ref),
    true = number(Float),
    true = number(42),
    false = number(Tuple),
    other = classify(atom),
    other = classify(42),
    true = ets:delete(T),
    ok.

classify(X) when is_tuple(X) -> tuple;
classify(X) when is_float(X) -> float;
classify(X) when is_map(X) -> map;
classify(X) when is_bitstring(X) -> bitstring;
classify(X) when is_reference(X) -> reference;
classify(_) -> other.

number(X) when is_number(X) -> true;
number(_) -> false.
