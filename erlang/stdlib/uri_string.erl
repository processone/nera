%% Project-owned RFC 3986 URI operations. No OTP or Cowlib source is vendored.
-module(uri_string).
-export([parse/1, normalize/1, recompose/1, resolve/3]).

parse(U) when is_binary(U); is_list(U) ->
    try parse_chars(chars(U), is_binary(U)) catch throw:E -> E end.
normalize(U) -> try recompose_checked(normalize_map(to_map(U))) catch throw:E -> E end.
recompose(M) when is_map(M) -> try recompose_checked(M) catch throw:E -> E end.
resolve(R, B, Options) ->
    try
        M = resolve_maps(to_map(R), to_map(B)),
        case Options of [] -> recompose_checked(M); [return_map] -> M end
    catch throw:E -> E end.
fail(Reason, Value) -> throw({error, Reason, Value}).
chars(B) when is_binary(B) -> utf8_chars(B);
chars(L) when is_list(L) -> chars_list(L).
utf8_chars(<<>>) -> [];
utf8_chars(<<C/utf8,T/binary>>) -> [C|utf8_chars(T)].
chars_list([]) -> [];
chars_list([H|T]) when is_integer(H) -> [H|chars_list(T)];
chars_list([H|T]) -> chars(H) ++ chars_list(T).
repr(L, true) -> unicode_binary(L);
repr(L, false) -> L.
unicode_binary(L) -> iolist_to_binary([<<C/utf8>> || C <- L]).
like(L, Old) -> repr(L, is_binary(Old)).
to_map(M) when is_map(M) -> check_map(M), M;
to_map(U) -> case parse(U) of M when is_map(M) -> M; E -> throw(E) end.
put_component(K, V, M, Bin) -> M#{K => repr(V,Bin)}.

split_at(C, L) -> split_at(C,L,[]).
split_at(_C, [], Acc) -> {lists:reverse(Acc), none};
split_at(C, [C|T], Acc) -> {lists:reverse(Acc),T};
split_at(C, [H|T], Acc) -> split_at(C,T,[H|Acc]).
parse_chars(L, Bin) ->
    {BeforeFrag, Frag} = split_at($#,L),
    {BeforeQuery, Query} = split_at($?,BeforeFrag),
    M0 = #{},
    M1 = optional(fragment,Frag,M0,Bin),
    M2 = optional(query,Query,M1,Bin),
    validate_opt(Frag, fragment), validate_opt(Query, query),
    {S, Rest} = scheme(BeforeQuery),
    M3 = optional(scheme,S,M2,Bin),
    case Rest of
        [$/,$/|T] ->
            {Authority, Path} = authority_path(T,[]),
            M4 = parse_authority(Authority,M3,Bin),
            validate(Path,path), put_component(path,Path,M4,Bin);
        _ ->
            validate(Rest,path),
            case S of none -> validate_relative(Rest); _ -> ok end,
            put_component(path,Rest,M3,Bin)
    end.
optional(_K,none,M,_B) -> M;
optional(K,V,M,B) -> put_component(K,V,M,B).
validate_opt(none,_Type) -> ok;
validate_opt(L,Type) -> validate(L,Type).
scheme(L) ->
    {Candidate, Tail} = split_at($:,L),
    case Tail =/= none andalso valid_scheme(Candidate) of
        true -> {Candidate,Tail}; false -> {none,L}
    end.
valid_scheme([H|T]) -> alpha(H) andalso lists:all(fun scheme_char/1,T);
valid_scheme([]) -> false.
scheme_char(C) -> alpha(C) orelse digit(C) orelse C =:= $+ orelse C =:= $- orelse C =:= $..
alpha(C) -> (C >= $a andalso C =< $z) orelse (C >= $A andalso C =< $Z).
digit(C) -> C >= $0 andalso C =< $9.
unreserved(C) -> alpha(C) orelse digit(C) orelse C =:= $- orelse C =:= $. orelse C =:= $_ orelse C =:= $~.
subdelim(C) -> lists:member(C,"!$&'()*+,;=").
allowed(C,Type) -> unreserved(C) orelse subdelim(C) orelse C =:= $% orelse
    (C =:= $: andalso Type =/= host) orelse
    (C =:= $@ andalso Type =/= host andalso Type =/= userinfo) orelse
    (C =:= $/ andalso (Type =:= path orelse Type =:= query orelse Type =:= fragment)) orelse
    (C =:= $? andalso (Type =:= query orelse Type =:= fragment)).
validate([],_) -> ok;
validate([H|T],Type) -> case allowed(H,Type) of true -> validate(T,Type); false -> fail(invalid_uri,[H]) end.
validate_relative(L) ->
    {First,_} = split_at($/,L),
    case lists:member($:,First) of true -> fail(invalid_uri,":"); false -> ok end.
authority_path([],Acc) -> {lists:reverse(Acc),[]};
authority_path([$/|T],Acc) -> {lists:reverse(Acc),[$/|T]};
authority_path([H|T],Acc) -> authority_path(T,[H|Acc]).
parse_authority(L,M,B) ->
    {User, AfterAt} = split_at($@,L),
    {HostPort,M1} = case AfterAt of none -> {L,M}; _ -> validate(User,userinfo), {AfterAt,put_component(userinfo,User,M,B)} end,
    case HostPort of
        [$[|T] ->
            {Host,After} = split_at($],T),
            case After =/= none andalso valid_ip(Host) of true -> ok; false -> fail(invalid_uri,":") end,
            M2=put_component(host,Host,M1,B),
            case After of [] -> M2; [$:|Port] -> port(Port,M2); _ -> fail(invalid_uri,":") end;
        _ ->
            {Host,P} = split_at($:,HostPort),
            case lists:all(fun(C)->allowed(C,host) end,Host) of true -> ok; false -> fail(invalid_uri,":") end,
            M2=put_component(host,Host,M1,B),
            case P of none -> M2; _ -> port(P,M2) end
    end.
valid_ip([$v|_]) -> false;
valid_ip(L) ->
    Parts=split_parts($:,L), Tokens=[P || P<-Parts,P =/= []],
    Units=lists:sum([ip_units(P) || P<-Tokens]),
    Valid=lists:all(fun(P)->ip_units(P) > 0 end,Tokens),
    case double_colons(L) of
        0 -> Valid andalso Units =:= 8 andalso length(Parts) =:= length(Tokens);
        1 -> Valid andalso Units < 8 andalso ends_ok(L) andalso ends_ok(lists:reverse(L));
        _ -> false
    end.
split_parts(C,L) -> case split_at(C,L) of {H,none}->[H]; {H,T}->[H|split_parts(C,T)] end.
double_colons([$:,$:,$:|_]) -> 2;
double_colons([$:,$:|T]) -> 1+double_colons(T);
double_colons([_|T]) -> double_colons(T);
double_colons([]) -> 0.
ends_ok([$:,$:|_]) -> true;
ends_ok([$:|_]) -> false;
ends_ok(_) -> true.
ip_units(P) ->
    case lists:member($.,P) of
        true -> Fields=split_parts($.,P),
            case length(Fields) =:= 4 andalso lists:all(fun valid_octet/1,Fields) of true->2; false->0 end;
        false -> case length(P) =< 4 andalso lists:all(fun(C)->hex(C) =/= bad end,P) of true->1; false->0 end
    end.
valid_octet([]) -> false;
valid_octet([$0,_|_]) -> false;
valid_octet(P) -> length(P) =< 3 andalso lists:all(fun digit/1,P) andalso list_to_integer(P) =< 255.

port([],M) -> M#{port=>undefined};
port(L,M) -> case lists:all(fun digit/1,L) of true -> M#{port=>list_to_integer(L)}; false -> fail(invalid_uri,":") end.

check_map(M) ->
    case maps:is_key(path,M) andalso lists:all(fun(K)->lists:member(K,[scheme,userinfo,host,port,path,query,fragment]) end,maps:keys(M)) of
        true -> ok; false -> fail(invalid_map,M)
    end.
all_binary(M) -> lists:all(fun({K,V})->K =:= port orelse is_binary(V) end,maps:to_list(M)).
get_chars(K,M) -> chars(maps:get(K,M,[])).
recompose_checked(M) ->
    check_map(M),
    case maps:is_key(port,M) andalso not maps:is_key(host,M) of true -> error(function_clause); false -> ok end,
    Scheme = case maps:find(scheme,M) of {ok,S} -> chars(S) ++ ":"; error -> [] end,
    Auth = case maps:find(host,M) of
        error -> [];
        {ok,H} ->
            Host=chars(H), HostText=case lists:member($:,Host) of true -> "["++Host++"]"; false -> quote(Host,host) end,
            User=case maps:find(userinfo,M) of {ok,U}->quote(chars(U),userinfo)++"@"; error->[] end,
            Port=case maps:find(port,M) of error->[]; {ok,undefined}->":"; {ok,N} when is_integer(N)->":"++integer_to_list(N) end,
            "//"++User++HostText++Port
    end,
    P0=get_chars(path,M),
    P=case {Auth,P0} of {[],_}->P0; {_,[]}->[]; {_,[$/|_]}->P0; _->[$/|P0] end,
    Q=component_suffix(query,$?,M), F=component_suffix(fragment,$#,M),
    repr(Scheme++Auth++quote(P,path)++Q++F,all_binary(M)).
component_suffix(K,Sep,M) -> case maps:find(K,M) of error->[]; {ok,V}->[Sep|quote(chars(V),K)] end.
quote([],_) -> [];
quote([C|T],Type) ->
    case allowed(C,Type) of
        true -> [C|quote(T,Type)];
        false -> percent_bytes(binary_to_list(<<C/utf8>>)) ++ quote(T,Type)
    end.
percent_bytes([]) -> [];
percent_bytes([B|T]) -> [$%,hex_digit(B bsr 4),hex_digit(B band 15)|percent_bytes(T)].
hex_digit(N) when N < 10 -> $0+N;
hex_digit(N) -> $A+N-10.
hex(C) when C >= $0,C =< $9 -> C-$0;
hex(C) when C >= $a,C =< $f -> C-$a+10;
hex(C) when C >= $A,C =< $F -> C-$A+10;
hex(_) -> bad.
lower(L) -> [case C >= $A andalso C =< $Z of true->C+32; false->C end || C<-L].
lower_uri([$%,A,B|T]) -> [$%,A,B|lower_uri(T)];
lower_uri([H|T]) -> lower([H]) ++ lower_uri(T);
lower_uri([]) -> [].
normalize_map(M) ->
    M0=lists:foldl(fun(K,A)->case maps:find(K,A) of error->A; {ok,V}->A#{K=>like(norm_percent(chars(V),V),V)} end end,M,[scheme,userinfo,host,path,query,fragment]),
    M1=lists:foldl(fun(K,A)->case maps:find(K,A) of error->A; {ok,V}->A#{K=>like(lower_uri(chars(V)),V)} end end,M0,[scheme,host]),
    Scheme=get_chars(scheme,M1),
    M2=case {Scheme,maps:get(port,M1,none)} of
        {_,undefined}->maps:remove(port,M1);
        {"http",80}->maps:remove(port,M1); {"https",443}->maps:remove(port,M1); {"ftp",21}->maps:remove(port,M1); _->M1 end,
    Old=maps:get(path,M2), P=dots(chars(Old),[]),
    P1=case {Scheme,P} of {"http",[]}->"/"; {"https",[]}->"/"; _->P end,
    M2#{path=>like(P1,Old)}.
norm_percent([],_) -> [];
norm_percent([$%,A,B|T],Old) ->
    case {hex(A),hex(B)} of
        {bad,_}->fail(invalid_percent_encoding,unicode_binary([$%,A,B|T]));
        {_,bad}->fail(invalid_percent_encoding,unicode_binary([$%,A,B|T]));
        {X,Y}->C=X*16+Y, case unreserved(C) of true->[C|norm_percent(T,Old)]; false->[$%,hex_digit(X),hex_digit(Y)|norm_percent(T,Old)] end
    end;
norm_percent([$%|T],_Old) -> [$%|T];
norm_percent([H|T],Old) -> [H|norm_percent(T,Old)].
dots([],Out) -> Out;
dots([$.,$.,$/|T],Out) -> dots(T,Out);
dots([$.,$/|T],Out) -> dots(T,Out);
dots([$/,$.,$/|T],Out) -> dots([$/|T],Out);
dots([$/,$.],Out) -> dots([$/],Out);
dots([$/,$.,$.,$/|T],Out) -> dots([$/|T],pop_segment(Out));
dots([$/,$.,$.],Out) -> dots([$/],pop_segment(Out));
dots(".",Out) -> Out;
dots("..",Out) -> Out;
dots(L,Out) -> {Segment,T}=next_segment(L), dots(T,Out++Segment).
next_segment([$/|T]) -> {S,Rest}=split_at($/,T), case Rest of none->{[$/|S],[]}; _->{[$/|S],[$/|Rest]} end;
next_segment(L) -> {S,Rest}=split_at($/,L), case Rest of none->{S,[]}; _->{S,[$/|Rest]} end.
pop_segment(Out) -> case split_at($/,lists:reverse(Out)) of {_,none}->[]; {_,T}->lists:reverse(T) end.

resolve_maps(R,B) ->
    case maps:is_key(scheme,R) of
        true -> clean_path(R);
        false ->
            case maps:is_key(scheme,B) of true->ok; false->fail(invalid_scheme,[]) end,
            Scheme=maps:get(scheme,B),
            case maps:is_key(host,R) of
                true -> clean_path(R#{scheme=>Scheme});
                false ->
                    RP=maps:get(path,R), BP=maps:get(path,B),
                    M=inherit([scheme,host,port,userinfo],B,R),
                    case chars(RP) of
                        [] -> M1=M#{path=>BP}, case maps:is_key(query,R) of true->M1; false->inherit([query],B,M1) end;
                        [$/|_] -> clean_path(M);
                        _ ->
                            BasePath=chars(BP),
                            Prefix=case {maps:is_key(host,B),BasePath} of {true,[]}->"/"; _->directory(BasePath) end,
                            M#{path=>like(dots(Prefix++chars(RP),[]),RP)}
                    end
            end
    end.
inherit([],_,M) -> M;
inherit([K|T],B,M) -> case maps:find(K,B) of error->inherit(T,B,M); {ok,V}->inherit(T,B,M#{K=>V}) end.
clean_path(M) -> Old=maps:get(path,M),M#{path=>like(dots(chars(Old),[]),Old)}.
directory(P) -> case split_at($/,lists:reverse(P)) of {_,none}->[]; {_,T}->lists:reverse([$/|T]) end.
