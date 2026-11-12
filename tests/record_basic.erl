-module(record_basic).
-export([test/0]).

-record(person, {name, age = 0}).
-record(point, {x = 0, y = 0, z = 0}).
-record(empty, {}).

%% Tests for record support (syntactic sugar over tagged tuples).

test() ->
    %% 1. Record creation with all fields
    P1 = #person{name = alice, age = 30},
    {person, alice, 30} = P1,

    %% 2. Record creation with defaults (missing age -> default 0)
    P2 = #person{name = bob},
    {person, bob, 0} = P2,

    %% 3. Record creation with no fields (all defaults)
    Pt0 = #point{},
    {point, 0, 0, 0} = Pt0,

    %% 4. Field access
    alice = P1#person.name,
    30 = P1#person.age,
    bob = P2#person.name,
    0 = P2#person.age,

    %% 5. Record update (single field)
    P3 = P1#person{age = 31},
    {person, alice, 31} = P3,

    %% 6. Record update (multiple fields)
    P4 = P1#person{name = carol, age = 25},
    {person, carol, 25} = P4,

    %% 7. Original unchanged after update
    {person, alice, 30} = P1,

    %% 8. Record index (#person.name -> integer)
    2 = #person.name,
    3 = #person.age,
    2 = #point.x,
    3 = #point.y,
    4 = #point.z,

    %% 9. Pattern matching - full record pattern
    #person{name = N1, age = A1} = P1,
    alice = N1,
    30 = A1,

    %% 10. Pattern matching - partial record pattern (age wildcarded)
    #person{name = N2} = P2,
    bob = N2,

    %% 11. Record in case expression
    Result = case P1 of
        #person{name = alice, age = Age} -> Age;
        #person{name = _} -> unknown
    end,
    30 = Result,

    %% 12. Missing name field -> undefined (default for fields without explicit default)
    P5 = #person{age = 99},
    {person, undefined, 99} = P5,

    %% 13. Empty record
    E = #empty{},
    {empty} = E,

    %% 14. Nested record operations
    Pt1 = #point{x = 10, y = 20, z = 30},
    10 = Pt1#point.x,
    Pt2 = Pt1#point{x = 100},
    {point, 100, 20, 30} = Pt2,

    io:format("ok~n"),
    ok.
