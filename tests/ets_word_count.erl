%% Word frequency counter inspired by Armstrong's "Programming Erlang".
%% Exercises: insert_new, update_counter, first/next iteration,
%%            tab2list, delete_all_objects.
-module(ets_word_count).
-export([test/0]).

test() ->
    test_simple_count(),
    test_paragraph(),
    test_top_words(),
    ok.

%% Count occurrences of words in a simple list
test_simple_count() ->
    T = ets:new(words, [set, public]),

    Words = [hello, world, hello, erlang, hello, world],
    count_words(T, Words),

    [{hello, 3}] = ets:lookup(T, hello),
    [{world, 2}] = ets:lookup(T, world),
    [{erlang, 1}] = ets:lookup(T, erlang),
    3 = ets:info(T, size),

    ets:delete(T),
    ok.

%% Longer document with repeated patterns
test_paragraph() ->
    T = ets:new(para, [set, public]),

    %% Simulate a paragraph of text as atom words
    Doc = [the, quick, brown, fox, jumps, over, the, lazy, dog,
           the, fox, runs, over, the, hill, also, the, dog, barks,
           the, quick, fox, jumps, again],
    count_words(T, Doc),

    %% "the" appears 6 times
    [{the, 6}] = ets:lookup(T, the),
    %% "fox" appears 3 times
    [{fox, 3}] = ets:lookup(T, fox),
    %% "quick" appears 2 times
    [{quick, 2}] = ets:lookup(T, quick),
    %% "dog" appears 2 times
    [{dog, 2}] = ets:lookup(T, dog),
    %% "again" appears 1 time
    [{again, 1}] = ets:lookup(T, again),

    %% Total unique words (the,quick,brown,fox,jumps,over,lazy,dog,runs,hill,also,barks,again)
    Words = ets:tab2list(T),
    13 = length(Words),

    %% Verify total word count sums to document length
    24 = sum_counts(T, ets:first(T), 0),

    ets:delete(T),
    ok.

%% Find the most frequent word by iterating
test_top_words() ->
    T = ets:new(top, [set, public]),

    Doc = [cat, dog, cat, bird, cat, dog, fish, cat, dog, bird],
    count_words(T, Doc),

    %% cat=4, dog=3, bird=2, fish=1
    [{cat, 4}] = ets:lookup(T, cat),
    [{dog, 3}] = ets:lookup(T, dog),
    [{bird, 2}] = ets:lookup(T, bird),
    [{fish, 1}] = ets:lookup(T, fish),

    %% Find the max via iteration
    {MaxWord, MaxCount} = find_max(T, ets:first(T), {none, 0}),
    cat = MaxWord,
    4 = MaxCount,

    %% Find the min via iteration
    {MinWord, MinCount} = find_min(T, ets:first(T), {none, 999999}),
    fish = MinWord,
    1 = MinCount,

    ets:delete(T),
    ok.

%% --- Helpers ---

count_words(_T, []) -> ok;
count_words(T, [W | Rest]) ->
    case ets:insert_new(T, {W, 1}) of
        true -> ok;
        false -> ets:update_counter(T, W, 1)
    end,
    count_words(T, Rest).

sum_counts(_T, '$end_of_table', Acc) -> Acc;
sum_counts(T, Key, Acc) ->
    [{_, Count}] = ets:lookup(T, Key),
    sum_counts(T, ets:next(T, Key), Acc + Count).

find_max(_T, '$end_of_table', Best) -> Best;
find_max(T, Key, {_, BestCount} = Best) ->
    [{_, Count}] = ets:lookup(T, Key),
    NewBest = case Count > BestCount of
        true -> {Key, Count};
        false -> Best
    end,
    find_max(T, ets:next(T, Key), NewBest).

find_min(_T, '$end_of_table', Best) -> Best;
find_min(T, Key, {_, BestCount} = Best) ->
    [{_, Count}] = ets:lookup(T, Key),
    NewBest = case Count < BestCount of
        true -> {Key, Count};
        false -> Best
    end,
    find_min(T, ets:next(T, Key), NewBest).
