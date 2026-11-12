-module(binary_float).
-export([test/0]).

id(X) -> X.

test() ->
    %% === Construction: 64-bit float (default) ===
    B1 = <<3.14:64/float>>,
    ok = check_byte_size(B1, 8),

    %% === Round-trip: construct then extract ===
    <<F1:64/float>> = B1,
    ok = check_float_eq(F1, 3.14),

    %% === Integer value in float segment ===
    B2 = <<42:64/float>>,
    <<F2:64/float>> = B2,
    ok = check_float_eq(F2, 42.0),

    %% === Negative float ===
    B3 = <<-1.5:64/float>>,
    <<F3:64/float>> = B3,
    ok = check_float_eq(F3, -1.5),

    %% === 32-bit float ===
    B4 = <<1.0:32/float>>,
    ok = check_byte_size(B4, 4),
    <<F4:32/float>> = B4,
    ok = check_float_eq_approx(F4, 1.0),

    %% === Little-endian float ===
    B5 = <<2.5:64/float-little>>,
    <<F5:64/float-little>> = B5,
    ok = check_float_eq(F5, 2.5),

    %% === Mixed segments: integer + float ===
    B6 = <<1, 2.718:64/float, 99>>,
    <<Tag:8, E:64/float, Tail:8>> = B6,
    1 = id(Tag),
    ok = check_float_eq(E, 2.718),
    99 = id(Tail),

    %% === Negative integer in float segment ===
    B6b = <<-42:64/float>>,
    <<F6b:64/float>> = B6b,
    ok = check_float_eq(F6b, -42.0),

    %% === Zero ===
    B7 = <<0.0:64/float>>,
    <<F7:64/float>> = B7,
    ok = check_float_eq(F7, 0.0),

    %% === Dynamic value through id() ===
    Val = id(9.99),
    B8 = <<Val:64/float>>,
    <<F8:64/float>> = B8,
    ok = check_float_eq(F8, 9.99),

    ok.

check_byte_size(Bin, Expected) ->
    Expected = byte_size(Bin),
    ok.

check_float_eq(A, B) ->
    true = (A == B),
    ok.

%% 32-bit floats have limited precision, allow small epsilon
check_float_eq_approx(A, B) ->
    Diff = abs(A - B),
    true = (Diff < 0.001),
    ok.

abs(X) when X < 0 -> -X;
abs(X) -> X.
