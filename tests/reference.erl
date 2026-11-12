-module(reference).
-export([test/0]).

test() ->
    %% make_ref creates unique references
    R1 = make_ref(),
    R2 = make_ref(),

    %% is_reference type check
    true = is_reference(R1),
    true = is_reference(R2),
    false = is_reference(42),
    false = is_reference(hello),
    false = is_reference(self()),
    false = is_reference([1,2]),
    false = is_reference({a, b}),

    %% Each reference is unique
    true = (R1 =/= R2),

    %% A reference equals itself
    true = (R1 =:= R1),
    true = (R2 =:= R2),

    %% References work in pattern matching
    R3 = make_ref(),
    R3 = id(R3),

    %% References can be stored in data structures
    T = {ref, R1},
    {ref, R1} = T,

    L = [R1, R2],
    [R1, R2] = L,

    ok.

id(X) -> X.
