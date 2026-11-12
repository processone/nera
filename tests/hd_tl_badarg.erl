%% Tests that hd/1 and tl/1 raise error:badarg on non-list arguments.
-module(hd_tl_badarg).
-export([test/0]).

test() ->
    %% hd on atom should raise badarg
    caught = try hd(id(not_a_list))
             catch error:badarg -> caught
             end,

    %% tl on atom should raise badarg
    caught = try tl(id(not_a_list))
             catch error:badarg -> caught
             end,

    %% hd on empty list should raise badarg
    caught = try hd(id([]))
             catch error:badarg -> caught
             end,

    %% tl on empty list should raise badarg
    caught = try tl(id([]))
             catch error:badarg -> caught
             end,

    %% hd on integer should raise badarg
    caught = try hd(id(123))
             catch error:badarg -> caught
             end,

    %% erlang:hd/1 explicit remote call
    caught = try erlang:hd(id(42))
             catch error:badarg -> caught
             end,

    %% Normal cases still work
    1 = hd(id([1, 2, 3])),
    [2, 3] = tl(id([1, 2, 3])),

    ok.

id(X) -> X.
