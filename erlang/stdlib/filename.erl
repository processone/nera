%% Project-owned subset of filename for Unix path semantics. No OTP
%% implementation is vendored.
-module(filename).
-export([extension/1]).

%% The extension is the text from the last dot of the final path component,
%% unless that dot starts the component. Binaries give binaries; any other
%% name (atoms and nested lists, atoms inside lists included) gives a list.
extension(Name) when is_binary(Name) ->
    case scan(Name, 0, 0, none) of
        none -> <<>>;
        Pos ->
            <<_:Pos/binary, Rest/binary>> = Name,
            Rest
    end;
extension(Name) ->
    case list_component(flatten(Name), []) of
        [] -> [];
        %% Skipping the first character keeps a leading dot from counting.
        [_ | Rest] -> last_dot_list(Rest, [])
    end.

%% Position of the last dot in the final component, ignoring a dot that
%% starts the component; a slash starts a new component.
scan(<<>>, _, _, Dot) -> Dot;
scan(<<$/, Rest/binary>>, Pos, _, _) -> scan(Rest, Pos+1, Pos+1, none);
scan(<<$., Rest/binary>>, Pos, Start, Dot) ->
    case Pos =:= Start of
        true -> scan(Rest, Pos+1, Start, Dot);
        false -> scan(Rest, Pos+1, Start, Pos)
    end;
scan(<<_, Rest/binary>>, Pos, Start, Dot) -> scan(Rest, Pos+1, Start, Dot).

%% The characters after the last slash; non-integers are a function_clause.
list_component([$/ | Rest], _) -> list_component(Rest, []);
list_component([C | Rest], Acc) when is_integer(C) -> list_component(Rest, [C | Acc]);
list_component([], Acc) -> lists:reverse(Acc).

last_dot_list([], Found) -> Found;
last_dot_list([$. | Tail] = Here, _) -> last_dot_list(Tail, Here);
last_dot_list([_ | Rest], Found) -> last_dot_list(Rest, Found).

flatten(Bin) when is_binary(Bin) -> Bin;
flatten(List) -> do_flatten(List, []).

do_flatten([H|T], Tail) when is_list(H) -> do_flatten(H, do_flatten(T, Tail));
do_flatten([H|T], Tail) when is_atom(H) -> atom_to_list(H) ++ do_flatten(T, Tail);
do_flatten([H|T], Tail) -> [H | do_flatten(T, Tail)];
do_flatten([], Tail) -> Tail;
do_flatten(Atom, Tail) when is_atom(Atom) -> atom_to_list(Atom) ++ flatten(Tail).
