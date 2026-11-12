%% EXPECT: group_leader_ok
-module(group_leader).
-export([test/0]).
test() ->
    Parent = self(), Original = group_leader(),
    true = is_pid(Original),
    Leader = spawn(fun() -> receive stop -> ok end end),
    true = group_leader(Leader, self()),
    Leader = group_leader(),
    {group_leader, Leader} = process_info(self(), group_leader),
    Pid = spawn(fun() -> Parent ! {leader, self(), group_leader()} end),
    receive {leader, Pid, Leader} -> ok after 1000 -> error(inheritance_failed) end,
    ok = try group_leader(not_pid, self()), error(unexpected) catch error:badarg -> ok end,
    true = group_leader(Original, self()),
    Leader ! stop,
    io:format("group_leader_ok~n").
