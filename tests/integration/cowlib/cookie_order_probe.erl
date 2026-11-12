%% Project-owned diagnostic: report raw attributes without normalizing them.
-module(cookie_order_probe).
-export([start/0]).

start() ->
    Cookie = iolist_to_binary(cow_cookie:setcookie(
        <<"sample">>, <<"value">>,
        #{http_only => true, domain => <<"example.org">>})),
    io:format("COOKIE_ORDER:~s~n", [Cookie]).
