-module(process_dict_get_all).
-export([test/0]).

test() ->
    Parent = self(),
    spawn_link(fun() ->
        [] = get(),
        undefined = put(alpha, one),
        undefined = put({key, [1, 2]}, {value, <<"payload">>}),
        Before = lists:sort(get()),
        Expected = lists:sort([{alpha, one}, {{key, [1, 2]}, {value, <<"payload">>}}]),
        Expected = Before,
        Expected = lists:sort(erlang:get()),
        Expected = lists:sort(apply(erlang, get, [])),
        one = put(alpha, two),
        [{alpha, two}] = begin
            {value, <<"payload">>} = erase({key, [1, 2]}),
            get()
        end,
        %% Earlier snapshots retain their original values.
        Expected = Before,
        ChildParent = self(),
        spawn_link(fun() ->
            [] = get(),
            undefined = put(alpha, child),
            [{alpha, child}] = get(),
            ChildParent ! child_done
        end),
        receive child_done -> ok after 5000 -> error(child_timeout) end,
        [{alpha, two}] = get(),
        two = erase(alpha),
        [] = get(),
        Parent ! dictionary_ok
    end),
    receive dictionary_ok -> ok after 5000 -> error(dictionary_timeout) end.
