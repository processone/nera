%% Tests: guards on binary patterns, byte_size in guards, combined guards.
-module(guard_binary).
-export([test/0]).

test() ->
    %% Guard on matched integer from binary
    big = classify_byte(<<200>>),
    small = classify_byte(<<10>>),
    zero = classify_byte(<<0>>),

    %% byte_size in guard
    long = check_length(<<1,2,3,4,5>>),
    short = check_length(<<1,2>>),
    empty = check_length(<<>>),

    %% Combined type + value guard
    {ok, 42} = extract_if_valid(<<1, 42>>),
    error = extract_if_valid(<<0, 42>>),
    error = extract_if_valid(<<1>>),

    %% Multiple clauses with binary guards
    first = multi_clause(<<1, 100>>),
    second = multi_clause(<<2, 50>>),
    other = multi_clause(<<3, 200>>),

    %% Guard on rest size
    has_payload = check_payload(<<0, 1, 2, 3>>),
    no_payload = check_payload(<<0>>),

    %% is_binary guard
    yes = is_bin_guard(<<1,2,3>>),
    no = is_bin_guard(hello),

    io:format("ok~n"),
    ok.

classify_byte(<<X>>) when X > 127 -> big;
classify_byte(<<X>>) when X > 0 -> small;
classify_byte(<<0>>) -> zero.

check_length(Bin) when byte_size(Bin) >= 4 -> long;
check_length(Bin) when byte_size(Bin) > 0 -> short;
check_length(<<>>) -> empty.

extract_if_valid(<<1, Value, _/binary>>) when Value > 0 -> {ok, Value};
extract_if_valid(_) -> error.

multi_clause(<<1, X>>) when X > 50 -> first;
multi_clause(<<2, X>>) when X >= 50 -> second;
multi_clause(<<_, _>>) -> other.

check_payload(<<_, Rest/binary>>) when byte_size(Rest) > 0 -> has_payload;
check_payload(<<_, _/binary>>) -> no_payload.

is_bin_guard(X) when is_binary(X) -> yes;
is_bin_guard(_) -> no.
