%% EXPECT: {'EXIT',bye} 'Foo'|{'EXIT',{'Elixir.Foo',undef}}|      'EXIT'|'hello world'|'Foo'|{'EXIT',...}|ok io_format_atom_quoting_ok
%% ~w, ~p, ~W and ~P quote atoms that need quotes, as OTP 29 does: Nera
%% printed 'EXIT', 'Foo' and 'hello world' bare, so a caught exit printed as
%% {EXIT,bye}. Expected values were taken from BEAM, where this module also
%% runs unchanged. Characters above 255 are escaped unless ~t is given.
-module(io_format_atom_quoting).
-export([test/0]).
test() ->
    %% A trapped exit as a linked process delivers it.
    process_flag(trap_exit, true),
    Pid = spawn_link(fun() -> exit(bye) end),
    Caught = receive {'EXIT', Pid, Reason} -> {'EXIT', Reason} end,
    %% io:format writes the same text io_lib:format returns.
    ok = io:format("~w ~p|", [Caught, 'Foo']),
    ok = io:format("~p|", [{'EXIT', {'Elixir.Foo', undef}}]),
    ok = io:format("~12w|~-13w|", ['EXIT', 'hello world']),
    ok = apply(io, id(format), ["~w|~W|", ['Foo', {'EXIT', [a, b]}, 2]]),
    "{'EXIT',bye}" = f("~w", [Caught]),
    "{'EXIT',bye}" = f("~p", [Caught]),
    "'EXIT'" = f("~w", ['EXIT']),
    "'Foo'" = f("~p", ['Foo']),
    "'hello world'" = f("~w", ['hello world']),
    %% Bare: lowercase Latin-1 start, then letters, digits, _ and @.
    "ok foo_Bar1 nonode@nohost [] true" = f("~w ~w ~w ~w ~w", [ok, foo_Bar1, nonode@nohost, [], true]),
    "'' '_x' '1a' '[]' '{}' 'a-b' 'node@host.example'" =
        f("~w ~w ~w ~w ~w ~w ~p", ['', '_x', '1a', '[]', '{}', 'a-b', 'node@host.example']),
    %% Reserved words, including maybe and else from the default maybe_expr.
    "'after' 'end' 'fun' 'cond' 'let' 'div' 'xor' 'maybe' 'else'" =
        f("~w ~w ~w ~w ~w ~w ~w ~w ~p", ['after', 'end', 'fun', 'cond', 'let', 'div', 'xor', 'maybe', 'else']),
    %% Quotes, backslashes and control characters are escaped inside quotes.
    "'it\\'s' 'back\\\\slash' 'nl\\nx' 'tab\\tx' 'del\\d' 'ctrl\\001' 'c1\\237'" =
        f("~w ~w ~w ~w ~w ~w ~p", ['it\'s', 'back\\slash', 'nl\nx', 'tab\tx', 'del\177', 'ctrl\001',
                                   list_to_atom([$c, $1, 159])]),
    %% Latin-1 letters: ß-ÿ start bare atoms, À-Þ continue them; ÷ and × do not.
    [228] = f("~w", [list_to_atom([228])]),
    [$x, 222] = f("~w", [list_to_atom([$x, 222])]),
    [$', 196, $b, $'] = f("~w", [list_to_atom([196, $b])]),
    [$', $x, 247, $'] = f("~p", [list_to_atom([$x, 247])]),
    [$', $x, 215, $'] = f("~w", [list_to_atom([$x, 215])]),
    %% Above 255: escaped by ~w and ~p, kept by ~tw and ~tp.
    Greek = list_to_atom([16#3B1, 16#3B2]),
    "'\\x{3B1}\\x{3B2}'" = f("~w", [Greek]),
    "'\\x{3B1}\\x{3B2}'" = f("~p", [Greek]),
    [$', 16#3B1, 16#3B2, $'] = f("~tw", [Greek]),
    [$', 16#3B1, 16#3B2, $'] = f("~tp", [Greek]),
    "{'\\x{1F600}'}" = f("~w", [{list_to_atom([16#1F600])}]),
    %% Nested in tuples, lists, maps and improper tails.
    "{'EXIT',self,['Foo',#{'K' => 'v w'}]}" = f("~w", [{'EXIT', self, ['Foo', #{'K' => 'v w'}]}]),
    "{'EXIT',self,['Foo',#{'K' => 'v w'}]}" = f("~p", [{'EXIT', self, ['Foo', #{'K' => 'v w'}]}]),
    "[a|'B']" = f("~w", [[a|'B']]),
    %% Field widths count the quotes; too narrow a field prints stars.
    "      'EXIT'" = f("~12w", ['EXIT']),
    "'EXIT'      " = f("~-12w", ['EXIT']),
    "**********" = f("~10w", ['hello world']),
    "'Foo'....." = f("~-10...w", ['Foo']),
    %% Depth-limited forms quote their leaves too.
    "{'EXIT',['Foo'|...]}" = f("~W", [{'EXIT', ['Foo', 'Bar']}, 4]),
    "{'EXIT',['Foo'|...]}" = f("~P", [{'EXIT', ['Foo', 'Bar']}, 4]),
    [${, $', 16#3B1, $', $,, $., $., $., $}] = f("~tW", [{list_to_atom([16#3B1]), x}, 2]),
    "{'\\x{3B1}',...}" = f("~P", [{list_to_atom([16#3B1]), x}, 2]),
    %% ~s still writes an atom's name without quotes.
    "Foo hello world" = f("~s ~s", ['Foo', 'hello world']),
    F = fun io_lib:format/2,
    "'EXIT'" = lists:flatten(F("~p", ['EXIT'])),
    io:format("ok io_format_atom_quoting_ok~n").
f(Fmt, Args) -> lists:flatten(io_lib:format(Fmt, Args)).
id(X) -> X.
