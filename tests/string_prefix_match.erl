-module(string_prefix_match).
-export([test/0]).

%% Test string prefix matching with ++ in patterns.

test() ->
    %% Basic prefix matching
    "world" = remove_prefix("hello world"),
    "hello world" = remove_prefix("hello world", ""),

    %% Multiple clauses with different prefixes
    {set, "bar"} = dispatch("set bar"),
    {get, "baz"} = dispatch("get baz"),
    {unknown, "other"} = dispatch("other"),

    %% Empty remainder
    {set, ""} = dispatch("set "),

    ok.

remove_prefix("hello " ++ Rest) -> Rest;
remove_prefix(Other) -> Other.

remove_prefix(Str, "") -> Str;
remove_prefix("hello " ++ Rest, _) -> Rest.

dispatch("set " ++ Rest) -> {set, Rest};
dispatch("get " ++ Rest) -> {get, Rest};
dispatch(Other) -> {unknown, Other}.
