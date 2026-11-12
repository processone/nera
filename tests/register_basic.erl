-module(register_basic).
-export([test/0]).

%% Test: register/2, whereis/1, unregister/1 basic behavior
test() ->
    %% Register self under a name
    Self = self(),
    true = register(my_name, Self),

    %% whereis returns the registered PID
    Self = whereis(my_name),

    %% whereis returns undefined for unregistered names
    undefined = whereis(no_such_name),

    %% unregister removes the name
    true = unregister(my_name),
    undefined = whereis(my_name),

    ok.
