-module(quoted_atom_escape).
-export([test/0]).

test() ->
    %% Quoted atom with escaped single quote
    'it\'s' = id('it\'s'),

    %% Quoted atom with newline escape
    'hello\nworld' = id('hello\nworld'),

    %% Quoted atom with tab escape
    'hello\tworld' = id('hello\tworld'),

    %% Quoted atom with backslash escape
    'back\\slash' = id('back\\slash'),

    %% Quoted atom with multiple escapes
    'a\tb\nc' = id('a\tb\nc'),

    %% Pattern match on escaped atoms
    ok = match_it('it\'s'),
    no = match_it(other),

    %% Pattern match in case
    yes = case id('hello\nworld') of
        'hello\nworld' -> yes;
        _ -> no
    end,

    %% Function clause matching on escaped atom
    found = lookup('it\'s'),
    not_found = lookup(something_else),

    ok.

match_it('it\'s') -> ok;
match_it(_) -> no.

lookup('it\'s') -> found;
lookup(_) -> not_found.

id(X) -> X.
