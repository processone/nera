%% Phone book example inspired by Armstrong's "Programming Erlang".
%% Exercises: named_table, insert, lookup, delete, member, tab2list,
%%            first/next iteration, keypos, update_element.
-module(ets_phonebook).
-export([test/0]).

test() ->
    test_basic_phonebook(),
    test_record_style(),
    test_bulk_operations(),
    test_iterate_contacts(),
    ok.

%% Simple phone book: {Name, Phone}
test_basic_phonebook() ->
    T = ets:new(phonebook, [named_table, set, public]),

    %% Add contacts
    ets:insert(phonebook, {joe, "+1-555-0101"}),
    ets:insert(phonebook, {mike, "+1-555-0102"}),
    ets:insert(phonebook, {robert, "+1-555-0103"}),
    ets:insert(phonebook, {jane, "+1-555-0104"}),

    %% Lookup a contact
    [{joe, "+1-555-0101"}] = ets:lookup(phonebook, joe),

    %% Check membership
    true = ets:member(phonebook, mike),
    false = ets:member(phonebook, nobody),

    %% Update a phone number
    ets:insert(phonebook, {joe, "+1-555-9999"}),
    [{joe, "+1-555-9999"}] = ets:lookup(phonebook, joe),

    %% Delete a contact
    ets:delete(phonebook, robert),
    [] = ets:lookup(phonebook, robert),
    false = ets:member(phonebook, robert),

    %% Remaining contacts still there
    true = ets:member(phonebook, joe),
    true = ets:member(phonebook, mike),
    true = ets:member(phonebook, jane),

    %% List all contacts
    All = ets:tab2list(phonebook),
    3 = length(All),
    true = lists:member({joe, "+1-555-9999"}, All),
    true = lists:member({mike, "+1-555-0102"}, All),
    true = lists:member({jane, "+1-555-0104"}, All),

    ets:delete(phonebook),
    ok.

%% Record-style with keypos: {phone, name, email}
%% Key is the name (position 2)
test_record_style() ->
    T = ets:new(contacts, [set, public, {keypos, 2}]),

    ets:insert(T, {"+1-555-0201", alice, "alice@example.com"}),
    ets:insert(T, {"+1-555-0202", bob, "bob@example.com"}),
    ets:insert(T, {"+1-555-0203", carol, "carol@example.com"}),

    %% Lookup by name (keypos = 2)
    [{"+1-555-0201", alice, "alice@example.com"}] = ets:lookup(T, alice),
    [{"+1-555-0202", bob, "bob@example.com"}] = ets:lookup(T, bob),

    %% Update email using update_element (position 3)
    ets:update_element(T, bob, {3, "bob.new@example.com"}),
    [{"+1-555-0202", bob, "bob.new@example.com"}] = ets:lookup(T, bob),

    %% insert_new fails for existing key
    false = ets:insert_new(T, {"+1-555-9999", alice, "alice2@example.com"}),
    %% Original preserved
    [{"+1-555-0201", alice, "alice@example.com"}] = ets:lookup(T, alice),

    %% insert_new succeeds for new key
    true = ets:insert_new(T, {"+1-555-0204", dave, "dave@example.com"}),
    [{"+1-555-0204", dave, "dave@example.com"}] = ets:lookup(T, dave),

    4 = ets:info(T, size),
    ets:delete(T),
    ok.

%% Batch insert and delete
test_bulk_operations() ->
    T = ets:new(bulk_pb, [set, public]),

    %% Insert a batch of contacts
    Contacts = [{ann, "111"}, {ben, "222"}, {cal, "333"},
                {dan, "444"}, {eve, "555"}, {fay, "666"}],
    ets:insert(T, Contacts),
    6 = ets:info(T, size),

    %% Delete some
    ets:delete(T, ann),
    ets:delete(T, cal),
    ets:delete(T, eve),
    3 = ets:info(T, size),

    %% Verify survivors
    true = ets:member(T, ben),
    true = ets:member(T, dan),
    true = ets:member(T, fay),
    false = ets:member(T, ann),

    %% Clear and verify
    ets:delete_all_objects(T),
    0 = ets:info(T, size),
    [] = ets:tab2list(T),

    ets:delete(T),
    ok.

%% Iterate through all contacts using first/next
test_iterate_contacts() ->
    T = ets:new(iter_pb, [set, public]),

    ets:insert(T, [{alpha, "a"}, {beta, "b"}, {gamma, "c"},
                   {delta, "d"}, {epsilon, "e"}]),

    %% Collect all keys via iteration
    Keys = collect_keys(T, ets:first(T), []),
    5 = length(Keys),
    true = lists:member(alpha, Keys),
    true = lists:member(beta, Keys),
    true = lists:member(gamma, Keys),
    true = lists:member(delta, Keys),
    true = lists:member(epsilon, Keys),

    %% Collect all values via iteration
    Values = collect_values(T, ets:first(T), []),
    5 = length(Values),
    true = lists:member("a", Values),
    true = lists:member("e", Values),

    ets:delete(T),
    ok.

collect_keys(_T, '$end_of_table', Acc) -> Acc;
collect_keys(T, Key, Acc) ->
    collect_keys(T, ets:next(T, Key), [Key | Acc]).

collect_values(_T, '$end_of_table', Acc) -> Acc;
collect_values(T, Key, Acc) ->
    [{_, Val}] = ets:lookup(T, Key),
    collect_values(T, ets:next(T, Key), [Val | Acc]).
