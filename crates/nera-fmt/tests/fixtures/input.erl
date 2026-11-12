-module(example).
-export([hello/0, add/2, factorial/1, handle/1, greet/1]).

%% A simple hello world function
hello() -> world.

%% Add two numbers
add(A,B) -> A+B.

%% Factorial function
factorial(0) -> 1;
factorial(N) when N > 0 -> N * factorial(N - 1).

%% Handle messages with case
handle(Msg) ->
  case Msg of
    {ok, Value} -> Value;
    {error, Reason} -> {failed, Reason};
    _ -> unknown
  end.

%% Greeting with pattern matching
greet(Name) ->
  Greeting = "Hello, ",
  Result = Greeting ++ Name,
  Result.
