-module(type_spec_attrs).
-export([test/0]).

%% Simple types
-type name() :: atom().
-type age() :: non_neg_integer().
-type pair(A, B) :: {A, B}.

%% Union types
-type result() :: {ok, term()} | {error, term()}.

%% Record type
-record(state, {count = 0, name}).
-type state() :: #state{}.

%% Integer range types (.. produces adjacent Dot tokens in the lexer)
-type byte_val() :: 0..255.
-type char_val() :: 0..16#10ffff.

%% Fun types including ellipsis (... produces three adjacent Dot tokens)
-type handler() :: fun((term()) -> ok | error).
-type any_fun() :: fun((...) -> term()).

%% Opaque type
-opaque internal() :: {secret, term()}.

%% Export types
-export_type([name/0, age/0]).

%% Behaviour and callback
-behaviour(gen_server).
-callback init(Args :: term()) -> {ok, term()}.
-callback handle_call(Request :: term(), From :: {pid(), term()}, State) ->
    {reply, term(), State} when State :: term().

%% Simple spec
-spec test() -> ok.

%% Spec with when constraint
-spec id(X) -> X when X :: term().

%% Multi-clause spec
-spec convert(integer()) -> binary();
             (binary()) -> integer().

%% Spec with complex types
-spec lookup(Key, Map) -> {ok, Value} | error when
      Key :: term(),
      Map :: #{term() => term()},
      Value :: term().

%% Deeply nested type
-type deep() :: {ok, [{atom(), fun((integer(), binary()) -> {ok, term()} | error)}]}.

%% Remote type references
-type server_ref() :: pid() | atom() | {atom(), node()}.

id(X) -> X.
convert(X) -> X.
lookup(K, M) -> maps:get(K, M).

test() ->
    a = id(a),
    42 = id(42),
    ok.
