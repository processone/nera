-module(process_dict).
-export([test/0]).

test() ->
    %% put/get basic
    undefined = put(mykey, 42),
    42 = get(mykey),

    %% put returns old value
    42 = put(mykey, 99),
    99 = get(mykey),

    %% get undefined key
    undefined = get(nokey),

    %% erase
    99 = erase(mykey),
    undefined = get(mykey),

    %% multiple keys
    undefined = put(a, 1),
    undefined = put(b, 2),
    undefined = put(c, 3),
    1 = get(a),
    2 = get(b),
    3 = get(c),

    %% get_keys
    Keys = get_keys(),
    3 = length(Keys),

    %% non-atom keys
    undefined = put(42, hello),
    hello = get(42),

    %% erase returns undefined for missing key
    undefined = erase(missing),

    ok.
