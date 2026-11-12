-module(preprocessor).
-export([start/0]).

%% Simple defines
-define(DEBUG).
-define(TIMEOUT, 5000).
-define(APP_NAME, "myapp").
-define(ADD(X, Y), X + Y).

%% Conditional compilation
-ifdef(DEBUG).
-define(LOG(X), io:format("~p~n", [X])).
-else.
-define(LOG(X), ok).
-endif.

%% Include directives
-include("header.hrl").
-include_lib("kernel/include/file.hrl").

%% Undef
-undef(DEBUG).

%% Define with complex body that should break
-define(LONG_MACRO(X, Y), case X of true -> Y; false -> error end).

%% Define with tuple body
-define(PAIR(A, B), {A, B}).

%% Directives with extra whitespace (should be normalized)
-ifdef(  TEST  ).
-endif.

start() -> ok.
