%% Tests list_to_existing_atom/1 and binary_to_existing_atom/2.
%% These BIFs must raise badarg for atoms not already in the atom table.
-module(existing_atom).
-export([test/0]).

test() ->
    %% Known atoms — should succeed (these are built-in atoms)
    ok = list_to_existing_atom("ok"),
    error = list_to_existing_atom("error"),
    true = list_to_existing_atom("true"),
    false = list_to_existing_atom("false"),

    %% Unknown atom — should raise badarg
    ok = try list_to_existing_atom("this_atom_surely_does_not_exist_xyz_42")
         catch error:badarg -> ok end,

    %% Non-list argument — should raise badarg
    ok = try list_to_existing_atom(123)
         catch error:badarg -> ok end,

    %% binary_to_existing_atom/2 — known atoms
    ok = binary_to_existing_atom(<<"ok">>, utf8),
    error = binary_to_existing_atom(<<"error">>, utf8),
    true = binary_to_existing_atom(<<"true">>, utf8),

    %% binary_to_existing_atom/2 — unknown atom should raise badarg
    ok = try binary_to_existing_atom(<<"no_such_atom_abc_99">>, utf8)
         catch error:badarg -> ok end,

    %% Roundtrip: create atom with list_to_atom, then find it with list_to_existing_atom
    _ = list_to_atom("dynamic_test_atom"),
    dynamic_test_atom = list_to_existing_atom("dynamic_test_atom"),

    ok.
