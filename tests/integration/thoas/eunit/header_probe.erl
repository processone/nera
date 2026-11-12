%% Header-resolution smoke test; this does not run the EUnit framework.
-module(header_probe).
-include_lib("eunit/include/eunit.hrl").
-export([test/0]).
-ifdef(ASSERT_HRL).
test() -> io:format("EUNIT_HEADERS_OK~n").
-else.
-error(assert_header_not_loaded).
-endif.
