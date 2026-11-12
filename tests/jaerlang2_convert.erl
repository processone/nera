%% Adapted from "Programming Erlang, 2nd Edition" by Joe Armstrong (convert1-5.erl)
%% Tests: float arithmetic, atom pattern matching, tuple patterns, nested case
-module(jaerlang2_convert).
-export([test/0]).

test() ->
    %% convert1: basic float arithmetic
    M = yards_to_meters(100),
    true = (M > 91.0),
    true = (M < 92.0),

    Y = meters_to_yards(100),
    true = (Y > 109.0),
    true = (Y < 110.0),

    %% convert2: atom dispatch
    M2 = convert(100, meters),
    true = (M2 > 91.0),

    Y2 = convert(100, yards),
    true = (Y2 > 109.0),

    %% convert3: tuple in, tuple out
    {meters, M3} = convert_t({yards, 100}, meters),
    true = (M3 > 91.0),

    {yards, Y3} = convert_t({meters, 100}, yards),
    true = (Y3 > 109.0),

    %% convert4: identity case (repeated variable in pattern)
    {meters, 50} = convert_id({meters, 50}, meters),

    %% convert5: nested case
    {feet, F1} = convert_nested({yards, 10}),
    true = (F1 > 29.0),
    true = (F1 < 31.0),

    ok.

%% convert1 style
yards_to_meters(Yards) -> 0.9144 * Yards.
meters_to_yards(Meters) -> 1.0936133 * Meters.

%% convert2 style
convert(Yards, meters) -> 0.9144 * Yards;
convert(Meters, yards) -> 1.0936133 * Meters.

%% convert3 style
convert_t({yards, X}, meters) -> {meters, 0.9144 * X};
convert_t({meters, X}, yards) -> {yards, 1.0936133 * X}.

%% convert4 style: identity when same tag
convert_id({yards, X}, meters)  -> {meters, 0.9144 * X};
convert_id({meters, X}, yards)  -> {yards, 1.0936133 * X};
convert_id({Tag, X}, Tag)       -> {Tag, X}.

%% convert5 style: nested case
convert_nested(In) ->
    case In of
        {yards, X} ->
            {feet, 3 * X};
        {meters, X} ->
            {feet, 3.2808399 * X};
        {feet, X} ->
            {feet, X}
    end.
