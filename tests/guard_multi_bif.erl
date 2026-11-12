%% Tests: guards with multiple BIF calls, andalso/orelse, guard sequences.
-module(guard_multi_bif).
-export([test/0]).

test() ->
    %% Multiple guards with comma (conjunction)
    yes = multi_guard([1, 2, 3]),
    no = multi_guard([]),
    no = multi_guard(not_a_list),

    %% andalso in guard
    ok = andalso_guard(5, 10),
    fail = andalso_guard(15, 10),
    fail = andalso_guard(not_int, 10),

    %% orelse in guard
    ok = orelse_guard(0, 5),
    ok = orelse_guard(5, 0),
    fail = orelse_guard(0, 0),

    %% Guard sequences (semicolon = disjunction)
    yes = guard_seq(42),
    yes = guard_seq(hello),
    no = guard_seq(3.14),
    no = guard_seq([]),

    %% Complex nested guard
    a = complex_guard({ok, [1,2,3]}),
    b = complex_guard({ok, []}),
    c = complex_guard({error, reason}),
    d = complex_guard(other),

    %% Guards with arithmetic
    pos = sign(42),
    neg = sign(-5),
    zero = sign(0),

    %% abs and comparison in guard
    close = within_range(5, 7, 3),
    far = within_range(5, 20, 3),

    %% Type checks combined
    int_atom = type_combo(42, hello),
    other_combo = type_combo(hello, 42),
    other_combo = type_combo(1.0, hello),

    %% Tuple guards
    {ok, 2} = safe_div(10, 5),
    error = safe_div(10, 0),
    error = safe_div(hello, 5),

    io:format("ok~n"),
    ok.

%% is_list + length + hd checks
multi_guard(X) when is_list(X), length(X) > 0, hd(X) == 1 -> yes;
multi_guard(_) -> no.

%% andalso
andalso_guard(X, Max) when is_integer(X) andalso X < Max -> ok;
andalso_guard(_, _) -> fail.

%% orelse
orelse_guard(X, Y) when X > 0 orelse Y > 0 -> ok;
orelse_guard(_, _) -> fail.

%% Guard sequences (;) — either clause can match
guard_seq(X) when is_integer(X), X > 10 -> yes;
guard_seq(X) when is_atom(X) -> yes;
guard_seq(_) -> no.

%% Complex: tuple + list + length
complex_guard({ok, L}) when is_list(L), length(L) > 0 -> a;
complex_guard({ok, L}) when is_list(L) -> b;
complex_guard({error, _}) -> c;
complex_guard(_) -> d.

%% Arithmetic in guards
sign(X) when is_integer(X), X > 0 -> pos;
sign(X) when is_integer(X), X < 0 -> neg;
sign(0) -> zero.

%% Guard with abs
within_range(X, Y, Tolerance) when abs(X - Y) =< Tolerance -> close;
within_range(_, _, _) -> far.

%% Multiple type checks
type_combo(A, B) when is_integer(A), is_atom(B) -> int_atom;
type_combo(_, _) -> other_combo.

%% Division safety
safe_div(A, B) when is_integer(A), is_integer(B), B =/= 0 -> {ok, A div B};
safe_div(_, _) -> error.
