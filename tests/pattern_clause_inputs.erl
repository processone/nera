%% EXPECT: pattern_clause_inputs_ok
-module(pattern_clause_inputs).
-export([test/0]).

%% Same-head rejection must preserve keys/sizes bound before the pattern and
%% sizes bound by an earlier segment of that same binary. Run also on BEAM.
test() ->
    Packet = id(<<8, 5>>),
    Message = id({route, #{route => ok}}),
    ok = head_key(id(#{route => ok})),
    5 = head_size(Packet),
    ok = case_key(route, Message),
    5 = case_size(Packet),
    ok = receive_key(route, Message),
    5 = receive_size(Packet),
    ok = of_key(route, Message),
    5 = of_size(Packet),
    ok = catch_key(route, Message),
    5 = catch_size(Packet),
    ok = else_key(route, Message),
    5 = else_size(Packet),
    ok = match_key(route, Message),
    5 = match_size(Packet),
    ok = maybe_match_key(route, Message),
    5 = maybe_match_size(Packet),
    ok = condmatch_key(route, Message),
    5 = condmatch_size(Packet),
    ok = scrutinee_key(id(#{route => ok})),
    ok = try_body_key(id(#{route => ok})),
    ok = rhs_key(id(#{route => ok})),
    ok = rhs_condmatch_key(id(#{route => ok})),
    ok = earlier_maybe_key(id(#{route => ok})),
    io:format("pattern_clause_inputs_ok~n"),
    ok.

head_key(#{route := V}) -> V.
head_size(<<N, V:N>>) -> V.

case_key(K, Message) ->
    case Message of {K, #{K := V}} -> V end.
case_size(Packet) ->
    case Packet of <<N, V:N>> -> V end.

receive_key(K, Message) ->
    self() ! Message,
    receive {K, #{K := V}} -> V after 1000 -> timeout end.
receive_size(Packet) ->
    self() ! Packet,
    receive <<N, V:N>> -> V after 1000 -> timeout end.

of_key(K, Message) ->
    try Message of {K, #{K := V}} -> V catch _:_ -> no end.
of_size(Packet) ->
    try Packet of <<N, V:N>> -> V catch _:_ -> no end.

catch_key(K, Message) ->
    try throw(Message) catch throw:{K, #{K := V}} -> V end.
catch_size(Packet) ->
    try throw(Packet) catch throw:<<N, V:N>> -> V end.

else_key(K, Message) ->
    maybe ok ?= Message else {K, #{K := V}} -> V end.
else_size(Packet) ->
    maybe ok ?= Packet else <<N, V:N>> -> V end.

match_key(K, Message) ->
    {K, #{K := V}} = Message,
    V.
match_size(Packet) ->
    <<N, V:N>> = Packet,
    V.

maybe_match_key(K, Message) ->
    maybe {K, #{K := V}} = Message, V end.
maybe_match_size(Packet) ->
    maybe <<N, V:N>> = Packet, V end.
condmatch_key(K, Message) ->
    maybe {K, #{K := V}} ?= Message, V end.
condmatch_size(Packet) ->
    maybe <<N, V:N>> ?= Packet, V end.

scrutinee_key(Map) ->
    case begin K = route, Map end of #{K := V} -> V end.
try_body_key(Map) ->
    try K = route, Map of #{K := V} -> V catch _:_ -> no end.
rhs_key(Map) ->
    #{K := V} = begin K = route, Map end,
    V.
rhs_condmatch_key(Map) ->
    maybe #{K := V} ?= begin K = route, Map end, V end.
earlier_maybe_key(Map) ->
    maybe begin K = route end, #{K := V} ?= Map, V end.

id(X) -> X.
