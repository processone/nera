%% EXPECT: float_integer_large_ok
%% Regression: trunc/1, round/1, ceil/1 and floor/1 returned 2^63 or -2^63
%% for floats beyond that range, instead of the exact integer. rand's
%% exs64, exsplus and exs1024 algorithms call trunc/1 on such floats for
%% ranges above 2^58 or 2^64. Expected values are OTP 29's. Run this fixture
%% on native Nera and BEAM.
-module(float_integer_large).
-export([test/0]).

test() ->
    E30 = 1000000000000000019884624838656,
    TwoPow63 = 9223372036854775808,
    MaxFloat = (1 bsl 1024) - (1 bsl 971),
    Cases =
        [{1.0e30, E30, E30, E30, E30},
         {-1.0e30, -E30, -E30, -E30, -E30},
         {9.2233720368547758e18, TwoPow63, TwoPow63, TwoPow63, TwoPow63},
         {-9.2233720368547758e18, -TwoPow63, -TwoPow63, -TwoPow63, -TwoPow63},
         {1.5e19, 15000000000000000000, 15000000000000000000,
          15000000000000000000, 15000000000000000000},
         {-9.3e18, -9300000000000000000, -9300000000000000000,
          -9300000000000000000, -9300000000000000000},
         {1.7976931348623157e308, MaxFloat, MaxFloat, MaxFloat, MaxFloat},
         %% Floats inside the small integer range keep their usual results.
         {2.5, 2, 3, 3, 2},
         {-2.5, -2, -3, -2, -3},
         {1.0e15 + 0.5, 1000000000000000, 1000000000000001,
          1000000000000001, 1000000000000000},
         {4.0e18, 4000000000000000000, 4000000000000000000,
          4000000000000000000, 4000000000000000000}],
    lists:foreach(
      fun({F, Trunc, Round, Ceil, Floor} = Case) ->
              Got = {F, trunc(id(F)), round(id(F)), ceil(id(F)), floor(id(F))},
              Case = Got,
              %% The same through apply/3 and remote funs.
              Trunc = apply(erlang, trunc, [F]),
              Round = (fun erlang:round/1)(F),
              Ceil = apply(erlang, ceil, [F]),
              Floor = (fun erlang:floor/1)(F)
      end, Cases),
    %% The results are integers equal to the float.
    true = trunc(id(1.0e30)) == 1.0e30,
    true = is_integer(round(id(-1.0e300))),
    io:format("float_integer_large_ok~n"),
    ok.

id(X) -> X.
