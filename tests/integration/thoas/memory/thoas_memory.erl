%% Project-owned probe: all measurements happen before leaving the tested VM.
-module(thoas_memory).
-export([start/0]).

start() ->
    lists:foreach(fun({Name, Options, CopyKey, CopyValue}) ->
        check(Name, Options, CopyKey, CopyValue)
    end, [{reference, #{}, false, false},
          {copy, #{keys => copy, strings => copy}, true, true},
          {keys_copy, #{keys => copy}, true, false},
          {strings_copy, #{strings => copy}, false, true}]),
    io:format("THOAS_MEMORY_DONE:4~n").

check(Name, Options, CopyKey, CopyValue) ->
    Key = binary:copy(<<"a">>, 101),
    Value = binary:copy(<<"b">>, 102),
    %% Detach builder capacity so the two VMs have an exact 210-byte parent.
    Input = binary:copy(iolist_to_binary([<<"{\"">>, Key, <<"\":\"">>, Value, <<"\"}">>])),
    {ok, Object} = thoas:decode(Input, Options),
    [{DecodedKey, DecodedValue}] = maps:to_list(Object),
    Key = DecodedKey,
    Value = DecodedValue,
    InputSize = byte_size(Input),
    InputSize = binary:referenced_byte_size(Input),
    ExpectedKey = case CopyKey of true -> byte_size(Key); false -> InputSize end,
    ExpectedValue = case CopyValue of true -> byte_size(Value); false -> InputSize end,
    ExpectedKey = binary:referenced_byte_size(DecodedKey),
    ExpectedValue = binary:referenced_byte_size(DecodedValue),
    io:format("THOAS_MEMORY:~w:~w:~w:~w~n", [Name, InputSize, ExpectedKey, ExpectedValue]).
