-module(records_maps).
-export([create_person/2, update_name/2, get_info/1, make_map/0]).

-record(person, {name, age}).

create_person(Name, Age) ->
  #person{name = Name, age = Age}.

update_name(P, NewName) ->
  P#person{name = NewName}.

get_info(#person{name = N, age = A}) ->
  {N, A}.

make_map() ->
  M = #{key => value, count => 0},
  M#{count := 1}.
