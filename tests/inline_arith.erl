-module(inline_arith).
-export([test/0]).

id(X) -> X.

test() ->
    %% Basic small-int arithmetic
    ok = test_add(),
    ok = test_sub(),
    ok = test_mul(),
    ok = test_div(),
    ok = test_rem(),
    ok = test_comparisons(),
    ok = test_mixed_types(),
    ok = test_is_integer(),
    ok = test_bitwise(),
    ok = test_shifts(),
    ok.

test_add() ->
    3 = id(1) + id(2),
    0 = id(0) + id(0),
    -1 = id(1) + id(-2),
    100 = id(99) + id(1),
    %% Large values near boundary
    X = 1 bsl 59,
    Y = X - 1 + X,  %% (2^59 - 1) + 2^59 = 2^60 - 1 = SMALL_INT_MAX
    Y = id(X - 1) + id(X),
    ok.

test_sub() ->
    1 = id(3) - id(2),
    0 = id(5) - id(5),
    -3 = id(0) - id(3),
    %% Negative result
    -100 = id(0) - id(100),
    ok.

test_mul() ->
    6 = id(2) * id(3),
    0 = id(0) * id(999),
    -6 = id(-2) * id(3),
    1 = id(1) * id(1),
    %% Overflow to bigint (2^30 * 2^30 = 2^60 which doesn't fit)
    Big = id(1 bsl 30) * id(1 bsl 30),
    true = Big > 0,
    true = Big =:= (1 bsl 60),
    ok.

test_div() ->
    3 = id(6) div id(2),
    0 = id(0) div id(5),
    -2 = id(-6) div id(3),
    1 = id(7) div id(7),
    %% Divide by zero should raise badarith
    ok = try id(1) div id(0) of
        _ -> not_ok
    catch
        error:badarith -> ok
    end,
    ok.

test_rem() ->
    1 = id(7) rem id(3),
    0 = id(6) rem id(3),
    -1 = id(-7) rem id(3),
    2 = id(5) rem id(3),
    %% Rem by zero should raise badarith
    ok = try id(1) rem id(0) of
        _ -> not_ok
    catch
        error:badarith -> ok
    end,
    ok.

test_comparisons() ->
    %% Lt
    true = id(1) < id(2),
    false = id(2) < id(1),
    false = id(1) < id(1),
    %% Gt
    true = id(2) > id(1),
    false = id(1) > id(2),
    false = id(1) > id(1),
    %% LtEq
    true = id(1) =< id(2),
    true = id(1) =< id(1),
    false = id(2) =< id(1),
    %% GtEq
    true = id(2) >= id(1),
    true = id(1) >= id(1),
    false = id(1) >= id(2),
    %% ExactEq
    true = id(1) =:= id(1),
    false = id(1) =:= id(2),
    %% ExactNotEq
    true = id(1) =/= id(2),
    false = id(1) =/= id(1),
    %% Eq (==)
    true = id(1) == id(1),
    false = id(1) == id(2),
    %% NotEq (/=)
    true = id(1) /= id(2),
    false = id(1) /= id(1),
    %% Negative numbers
    true = id(-5) < id(0),
    true = id(0) > id(-5),
    true = id(-5) =:= id(-5),
    false = id(-5) =:= id(5),
    ok.

test_mixed_types() ->
    %% Int + Float (should fall back to FFI)
    3.0 = id(1) + id(2.0),
    true = id(1) < id(2.0),
    true = id(1) == id(1.0),
    false = id(1) =:= id(1.0),
    ok.

test_is_integer() ->
    true = is_integer(id(42)),
    true = is_integer(id(-1)),
    true = is_integer(id(0)),
    false = is_integer(id(1.0)),
    false = is_integer(id(hello)),
    false = is_integer(id([])),
    %% Bigint should also be integer
    Big = id(1 bsl 60),
    true = is_integer(Big),
    ok.

test_bitwise() ->
    %% band basics
    2 = id(3) band id(6),
    0 = id(5) band id(0),
    255 = id(255) band id(255),
    0 = id(0) band id(0),
    %% bor basics
    7 = id(3) bor id(6),
    5 = id(5) bor id(0),
    255 = id(255) bor id(255),
    0 = id(0) bor id(0),
    %% bxor basics
    5 = id(3) bxor id(6),
    5 = id(5) bxor id(0),
    0 = id(255) bxor id(255),
    0 = id(0) bxor id(0),
    %% Negative numbers
    -8 = id(-3) band id(-6),
    -1 = id(-3) bor id(-6),
    7 = id(-3) bxor id(-6),
    %% Mixed positive/negative
    0 = id(-1) band id(0),
    -1 = id(-1) bor id(0),
    -1 = id(-1) bxor id(0),
    4 = id(-4) band id(7),
    -1 = id(-4) bor id(7),
    -5 = id(-4) bxor id(7),
    %% Powers of 2
    0 = id(16) band id(15),
    31 = id(16) bor id(15),
    31 = id(16) bxor id(15),
    %% Large small-int values
    X = (1 bsl 59) - 1,
    X = id(X) band id(X),
    X = id(X) bor id(X),
    0 = id(X) bxor id(X),
    %% Identity properties
    42 = id(42) band id(-1),
    42 = id(42) bor id(0),
    42 = id(42) bxor id(0),
    %% Self-xor = 0
    0 = id(12345) bxor id(12345),
    0 = id(-999) bxor id(-999),
    ok.

test_shifts() ->
    %% bsl basics
    8 = id(1) bsl id(3),
    0 = id(0) bsl id(10),
    -8 = id(-1) bsl id(3),
    1024 = id(1) bsl id(10),
    16 = id(2) bsl id(3),
    %% bsr basics
    1 = id(8) bsr id(3),
    0 = id(1) bsr id(3),
    -1 = id(-1) bsr id(3),
    -1 = id(-8) bsr id(3),
    4 = id(16) bsr id(2),
    %% Shift by 0 (identity)
    42 = id(42) bsl id(0),
    42 = id(42) bsr id(0),
    -7 = id(-7) bsl id(0),
    -7 = id(-7) bsr id(0),
    %% Large shift left (overflow to bigint via FFI)
    Big = id(1) bsl id(60),
    true = Big > 0,
    true = is_integer(Big),
    %% Shift by 59 (still fits for value 1)
    Half = id(1) bsl id(59),
    true = Half > 0,
    true = is_integer(Half),
    %% Large bsr (result → 0 or -1)
    0 = id(1) bsr id(50),
    0 = id(100) bsr id(50),
    -1 = id(-1) bsr id(50),
    -1 = id(-100) bsr id(50),
    %% bsl + bsr roundtrip
    V = id(12345),
    V = (id(V) bsl id(5)) bsr id(5),
    %% Shift negative by various amounts
    -16 = id(-2) bsl id(3),
    -32 = id(-4) bsl id(3),
    ok.
