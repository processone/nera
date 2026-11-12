%% EXPECT: bin_comp_bits_ok
%% Regression: a binary comprehension whose body yields bitstrings that are
%% not whole bytes packs them bit by bit, as on BEAM. Before the fix, the
%% comprehension finished with iolist_to_binary/1, so
%% << <<X:4>> || <<X>> <= <<1,2>> >> and << <<B:1>> || B <- [1,0,1] >>
%% raised badarg instead of returning <<18>> and <<5:3>>.
%% Run this same fixture on native Nera and BEAM.
-module(bin_comp_bits).
-export([test/0]).

test() ->
    %% The reported cases.
    <<18>> = << <<X:4>> || <<X>> <= id(<<1, 2>>) >>,
    <<5:3>> = << <<B:1>> || B <- id([1, 0, 1]) >>,

    %% A result that ends inside a byte is a bitstring, not a binary.
    Bits = << <<B:1>> || B <- id([1, 0, 1]) >>,
    3 = bit_size(Bits),
    false = is_binary(Bits),
    true = is_bitstring(Bits),
    true = is_binary(<< <<X:4>> || X <- id([1, 2]) >>),
    <<18, 3:4>> = << <<X:4>> || X <- id([1, 2, 3]) >>,
    <<>> = << <<B:1>> || B <- id([]) >>,

    %% Filters, several generators and bodies with several segments.
    <<5:3>> = << <<B:1>> || B <- id([1, 2, 0, 2, 1]), B =/= 2 >>,
    <<231, 200, 7:4>> = << <<X:3, Y:2>> || X <- id([7, 1]), Y <- id([0, 3]) >>,

    %% Values that cross byte boundaries.
    <<16#AB, 16#CD, 16#EF>> = << <<X:12>> || X <- id([16#ABC, 16#DEF]) >>,

    %% Bodies that are bitstring values, partial and whole bytes mixed.
    <<34>> = << X || X <- id([<<1:3>>, <<2:5>>]) >>,
    <<1:4, "ab", 1:4>> = << (piece(X)) || X <- id([1, 2, 1]) >>,

    %% A bitstring source read one bit at a time.
    <<5:3>> = << <<B:1>> || <<B:1>> <= id(<<5:3>>) >>,

    %% A nested comprehension as the body.
    <<63:6>> = << << <<B:1>> || B <- id([1, 1]) >> || _ <- id([a, b, c]) >>,

    %% Results above the inline binary size keep every bit.
    Ones = << <<1:1>> || _ <- lists:seq(1, id(1001)) >>,
    1001 = bit_size(Ones),
    true = Ones =:= <<((1 bsl 1001) - 1):1001>>,
    Sevens = << <<X:7>> || X <- lists:seq(0, id(100)) >>,
    707 = bit_size(Sevens),
    true = [X || <<X:7>> <= Sevens] =:= lists:seq(0, 100),

    %% A large body that starts inside a byte of a larger binary.
    Big = id(list_to_binary(lists:duplicate(100, 16#A5))),
    <<_:3, Rest/bits>> = Big,
    797 = bit_size(Rest),
    Twice = << R || R <- id([Rest, Rest]) >>,
    true = Twice =:= <<Rest/bits, Rest/bits>>,

    %% A large body that shares a larger binary's storage from a byte offset,
    %% placed once on a byte boundary and once after a single bit.
    Shared = tail(Big),
    Joined = << S || S <- id([Shared, <<1:1>>, Shared]) >>,
    true = Joined =:= <<Shared/binary, 1:1, Shared/binary>>,

    io:format("bin_comp_bits_ok~n"),
    ok.

piece(1) -> <<1:4>>;
piece(2) -> <<"ab">>.

tail(<<_:8, Tail/binary>>) -> Tail.

id(X) -> X.
