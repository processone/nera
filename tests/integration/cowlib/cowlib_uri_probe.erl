%% Project-owned assertions that execute the unchanged external cow_link module.
-module(cowlib_uri_probe).
-export([test/0]).
test() ->
    #{target := <<"https://example/b/~">>} = cow_link:resolve_link(
        #{target => <<"HTTPS://EXAMPLE:443/a/../b/%7e">>}, undefined),
    false = cow_link:resolve_link(#{target => <<"relative/path">>}, undefined),
    false = cow_link:resolve_link(#{target => <<"a b">>}, undefined),
    #{target := <<"http://example/a/next">>, attributes := []} = cow_link:resolve_link(
        #{target => <<"./next">>, attributes => []}, <<"http://example/a/index?old">>),
    #{target := <<"http://example/root#part">>} = cow_link:resolve_link(
        #{target => <<"/root#part">>, attributes => []}, <<"http://example/a/index">>),
    #{target := <<"http://example/dir/next">>} = cow_link:resolve_link(
        #{target => <<"next">>, attributes => [{<<"anchor">>, <<"../dir/context">>}]},
        <<"http://example/a/index">>),
    false = cow_link:resolve_link(
        #{target => <<"next">>, attributes => [{<<"anchor">>, <<"../dir/context">>}]},
        <<"http://example/a/index">>, #{allow_anchor => false}),
    #{target := <<"https://other/r">>} = cow_link:resolve_link(
        #{target => <<"//other/r">>, attributes => []}, <<"https://example/a/index">>),
    #{target := <<"http://h/a?new">>} = cow_link:resolve_link(
        #{target => <<"?new">>, attributes => []}, <<"http://h/a?old">>),
    io:format("ok~n"), ok.
