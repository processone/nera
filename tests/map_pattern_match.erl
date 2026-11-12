-module(map_pattern_match).
-export([test/0]).

%% Tests for := in map patterns within match expressions (= operator)

test() ->
    %% 1. Basic map pattern match with :=
    M = #{a => 1, b => 2},
    #{a := A} = M,
    1 = A,

    %% 2. Multiple fields
    #{a := A2, b := B2} = M,
    1 = A2,
    2 = B2,

    %% 3. Nested map pattern
    M2 = #{x => #{y => 42}},
    #{x := #{y := V}} = M2,
    42 = V,

    %% 4. In case clause (should already work)
    R = case M of
        #{a := Val} -> Val
    end,
    1 = R,

    %% 5. Map pattern in function head
    3 = extract_c(#{c => 3, d => 4}),

    io:format("ok~n"),
    ok.

extract_c(#{c := C}) -> C.
