-module(pid_domain_roundtrip).
-export([test/0]).

test() ->
    Bytes = <<131, 88, 119, 13, "nonode@nohost", 4294967295:32, 268435455:32, 0:32>>,
    Pid = binary_to_term(Bytes),
    true = is_pid(Pid),
    Bytes = term_to_binary(Pid),
    false = is_process_alive(Pid),
    self() ! {pid, Pid},
    receive {pid, Pid} -> ok after 1000 -> error(pid_copy_failed) end,
    Bad = <<131, 88, 119, 13, "nonode@nohost", 0:32, 268435456:32, 0:32>>,
    {'EXIT', {badarg, _}} = (catch binary_to_term(Bad)),
    Parent = self(),
    Child = spawn(fun() -> Parent ! {copied, Pid} end),
    true = is_pid(Child),
    receive {copied, Pid} -> ok after 1000 -> error(pid_capture_failed) end,
    ok.
