%% EXPECT: bif_node_ok
%% NERA-66: Ranch ranch_conns_sup:87 calls node(SupPid) in a nodedown exit.
%% Run the same assertions natively and with erlc/erl (BEAM), without distribution.
-module(bif_node).
-export([test/0]).

test() ->
    nonode@nohost = node(),
    nonode@nohost = erlang:node(),
    nonode@nohost = apply(id(erlang), id(node), []),
    Node = fun erlang:node/0,
    nonode@nohost = Node(),
    LocalNode = fun node/0,
    nonode@nohost = LocalNode(),
    Self = self(),
    {nodedown, nonode@nohost} = try exit({nodedown, node(Self)})
        catch exit:RanchReason -> RanchReason end,
    {Pid, Monitor} = spawn_monitor(fun() -> receive stop -> ok end end),
    check_identifiers([Self, Pid, make_ref(), Monitor, atomics:new(1, [])]),
    Pid ! stop,
    receive {'DOWN', Monitor, process, Pid, normal} -> ok
    after 1000 -> error(missing_down)
    end,
    %% node/1 describes origin, not liveness; monitor references remain valid.
    check_identifiers([Pid, Monitor]),
    check_badargs([not_an_identifier, nonode@nohost, 0, -1, 1 bsl 100,
                   1.0, [], [a], [a|b], {}, {Self}, #{pid => Self},
                   <<>>, <<1,2>>, <<1:1>>, fun() -> ok end, fun erlang:node/1]),
    %% Guard errors must not leak into following body calls.
    nonode@nohost = node(Self),
    io:format("bif_node_ok~n"),
    ok.

check_identifiers([]) -> ok;
check_identifiers([Term|Rest]) ->
    nonode@nohost = node(Term),
    nonode@nohost = erlang:node(Term),
    nonode@nohost = apply(id(erlang), id(node), [Term]),
    F = fun erlang:node/1,
    nonode@nohost = F(Term),
    G = fun node/1,
    nonode@nohost = G(Term),
    true = local(Term),
    true = qualified_local(Term),
    false = remote(Term),
    false = negated_local(Term),
    check_identifiers(Rest).

check_badargs([]) -> ok;
check_badargs([Term|Rest]) ->
    badarg = reason(fun() -> node(Term) end),
    badarg = reason(fun() -> erlang:node(Term) end),
    badarg = reason(fun() -> apply(id(erlang), id(node), [Term]) end),
    F = fun erlang:node/1,
    badarg = reason(fun() -> F(Term) end),
    G = fun node/1,
    badarg = reason(fun() -> G(Term) end),
    false = local(Term),
    false = qualified_local(Term),
    %% Invalid terms fail guards even under inequality, not, or orelse true.
    false = remote(Term),
    false = negated_local(Term),
    false = guard_orelse(Term),
    recovered = alternative(Term),
    check_badargs(Rest).

%% Both node arities are legal guard BIFs; exceptions reject the whole guard.
local(Pid) when node(Pid) =:= node() -> true;
local(_) -> false.
qualified_local(Term) when erlang:node(Term) =:= erlang:node() -> true;
qualified_local(_) -> false.
remote(Term) when node(Term) =/= node() -> true;
remote(_) -> false.
negated_local(Term) when not (node(Term) =:= node()) -> true;
negated_local(_) -> false.
guard_orelse(Term) when node(Term) =:= node() orelse true -> true;
guard_orelse(_) -> false.
alternative(Term) when node(Term) =:= node(); true -> recovered.

reason(F) -> try F() of V -> {unexpected, V} catch error:R -> R end.
id(X) -> X.
