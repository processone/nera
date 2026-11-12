-module(myapp_sup).
-behaviour(supervisor).

-export([start_link/0]).
-export([init/1]).

start_link() ->
    supervisor:start_link({local, ?MODULE}, ?MODULE, []).

init([]) ->
    {ok, {
        {one_for_one, 3, 10},
        [
            {myapp_worker,
             {myapp_worker, start_link, []},
             permanent,
             5000,
             worker,
             [myapp_worker]}
        ]
    }}.
