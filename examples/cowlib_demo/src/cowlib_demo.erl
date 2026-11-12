%% Mini example project: exercise Cowlib 2.22.0's public modules and
%% print one deterministic line per check, so the same program can be
%% run on Nera and on OTP and the two outputs compared line by line.
-module(cowlib_demo).
-export([start/0, start_canonical/0, checks/0]).

start() ->
    Results = [run(Name, Fun) || {Name, Fun} <- checks()],
    Ok = length([x || ok <- Results]),
    Failed = length(Results) - Ok,
    io:format("SUMMARY ~w checks, ~w returned, ~w raised~n",
              [length(Results), Ok, Failed]).

run(Name, Fun) ->
    try Fun() of
        Value ->
            io:format("~s ok ~w~n", [Name, Value]),
            ok
    catch
        Class:Reason ->
            io:format("~s EXC ~w:~w~n", [Name, Class, Reason]),
            error
    end.

bin(IoData) -> iolist_to_binary(IoData).

%% Same checks, but each result is printed in a canonical form: map
%% entries sorted, atoms spelled out. This separates "same value" from
%% "same printed order", since map iteration order is not specified.
start_canonical() ->
    Results = [run(Name, fun() -> canon(Fun()) end)
               || {Name, Fun} <- checks() ++ semantic_checks()],
    Ok = length([x || ok <- Results]),
    io:format("SUMMARY ~w checks, ~w returned, ~w raised~n",
              [length(Results), Ok, length(Results) - Ok]).

canon(M) when is_map(M) ->
    {map, lists:sort([{canon(K), canon(V)} || {K, V} <- maps:to_list(M)])};
canon(A) when is_atom(A) -> {atom, atom_to_binary(A, utf8)};
canon([H | T]) -> [canon(H) | canon(T)];
canon(T) when is_tuple(T) -> list_to_tuple([canon(X) || X <- tuple_to_list(T)]);
canon(X) -> X.

%% Outputs whose bytes follow map iteration order, compared by meaning.
semantic_checks() ->
    [{"sem.setcookie_attributes", fun() ->
          lists:sort(binary:split(
                       bin(cow_cookie:setcookie(<<"id">>, <<"42">>,
                                                #{path => <<"/">>, max_age => 3600,
                                                  http_only => true, secure => true,
                                                  same_site => strict})),
                       <<"; ">>, [global]))
      end},
     {"sem.settings_frame_reparsed", fun() ->
          Settings = #{max_concurrent_streams => 100, initial_window_size => 65535},
          {ok, {settings, Parsed}, <<>>} = cow_http2:parse(bin(cow_http2:settings(Settings))),
          Parsed =:= Settings
      end},
     {"sem.pseudo_headers_sorted", fun() ->
          lists:sort(cow_http:merge_pseudo_headers(
                       #{method => <<"GET">>, scheme => <<"https">>,
                         authority => <<"example.org">>, path => <<"/">>},
                       [{<<"accept">>, <<"*/*">>}]))
      end},
     {"sem.version_atom", fun() ->
          element(3, cow_http1:parse_request_line(<<"GET / HTTP/1.1\r\n">>)) =:= 'HTTP/1.1'
      end},
     {"fmt.quoted_atom_w", fun() -> lists:flatten(io_lib:format("~w", ['HTTP/1.1'])) end},
     {"fmt.quoted_atom_p", fun() -> lists:flatten(io_lib:format("~p", ['HTTP/1.1'])) end}].

checks() ->
    [
     %% ---- cow_qs: query strings -------------------------------------
     {"qs.parse", fun() ->
          cow_qs:parse_qs(<<"name=Nera&lang=erlang&empty&q=caf%C3%A9+au+lait">>)
      end},
     {"qs.build", fun() ->
          cow_qs:qs([{<<"a">>, <<"b c">>}, {<<"x">>, true}, {<<"é">>, <<"&=">>}])
      end},
     {"qs.urldecode", fun() -> cow_qs:urldecode(<<"hello+world%21%2F">>) end},
     {"qs.urlencode", fun() -> cow_qs:urlencode(<<"a b/c?d=é">>) end},

     %% ---- cow_uri ---------------------------------------------------
     {"uri.urlencode", fun() -> cow_uri:urlencode(<<"a b/c?d=é">>) end},
     {"uri.urldecode", fun() -> cow_uri:urldecode(<<"a%20b%2Fc">>) end},

     %% ---- cow_base64url ---------------------------------------------
     {"base64url.encode", fun() -> cow_base64url:encode(<<255, 254, 253, "nera">>) end},
     {"base64url.encode_padding", fun() ->
          cow_base64url:encode(<<"ab">>, #{padding => true})
      end},
     {"base64url.roundtrip", fun() ->
          cow_base64url:decode(cow_base64url:encode(<<0, 1, 2, 250, 251>>))
      end},

     %% ---- cow_date ----------------------------------------------------
     {"date.rfc7231", fun() -> cow_date:rfc7231({{2026, 11, 12}, {9, 5, 7}}) end},
     {"date.rfc1123", fun() -> cow_date:rfc1123({{1994, 11, 6}, {8, 49, 37}}) end},
     {"date.rfc2109", fun() -> cow_date:rfc2109({{2026, 1, 2}, {3, 4, 5}}) end},
     {"date.parse_imf", fun() -> cow_date:parse_date(<<"Sun, 06 Nov 1994 08:49:37 GMT">>) end},
     {"date.parse_rfc850", fun() -> cow_date:parse_date(<<"Sunday, 06-Nov-94 08:49:37 GMT">>) end},
     {"date.parse_asctime", fun() -> cow_date:parse_date(<<"Sun Nov  6 08:49:37 1994">>) end},

     %% ---- cow_cookie --------------------------------------------------
     {"cookie.parse", fun() ->
          cow_cookie:parse_cookie(<<"session=abc123; theme=dark; lang=fr">>)
      end},
     {"cookie.parse_set_cookie", fun() ->
          cow_cookie:parse_set_cookie(<<"id=a3fWa; Expires=Wed, 21 Oct 2015 07:28:00 GMT; "
                                        "Path=/docs; Secure; HttpOnly; SameSite=Lax">>)
      end},
     {"cookie.setcookie", fun() ->
          bin(cow_cookie:setcookie(<<"id">>, <<"42">>,
                                   #{path => <<"/">>, max_age => 3600, http_only => true,
                                     secure => true, same_site => strict}))
      end},
     {"cookie.cookie", fun() ->
          bin(cow_cookie:cookie([{<<"a">>, <<"1">>}, {<<"b">>, <<"2">>}]))
      end},

     %% ---- cow_http_hd: header parsing and building ------------------
     {"hd.accept", fun() ->
          cow_http_hd:parse_accept(<<"text/html;q=0.9, application/json, */*;q=0.1">>)
      end},
     {"hd.accept_language", fun() ->
          cow_http_hd:parse_accept_language(<<"fr-FR, fr;q=0.9, en;q=0.5">>)
      end},
     {"hd.accept_encoding", fun() ->
          cow_http_hd:parse_accept_encoding(<<"gzip, deflate;q=0.5, br">>)
      end},
     {"hd.content_type", fun() ->
          cow_http_hd:parse_content_type(<<"text/html; charset=UTF-8">>)
      end},
     {"hd.content_length", fun() -> cow_http_hd:parse_content_length(<<"1024">>) end},
     {"hd.cache_control", fun() ->
          cow_http_hd:parse_cache_control(<<"no-cache, max-age=600, private=\"x-foo\"">>)
      end},
     {"hd.connection", fun() -> cow_http_hd:parse_connection(<<"keep-alive, Upgrade">>) end},
     {"hd.host", fun() -> cow_http_hd:parse_host(<<"example.org:8080">>) end},
     {"hd.range", fun() -> cow_http_hd:parse_range(<<"bytes=0-499, 1000-">>) end},
     {"hd.etag", fun() -> cow_http_hd:parse_etag(<<"W/\"xyzzy\"">>) end},
     {"hd.if_none_match", fun() ->
          cow_http_hd:parse_if_none_match(<<"\"xyzzy\", W/\"r2d2xxxx\"">>)
      end},
     {"hd.authorization_basic", fun() ->
          cow_http_hd:parse_authorization(<<"Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ==">>)
      end},
     {"hd.authorization_bearer", fun() ->
          cow_http_hd:parse_authorization(<<"Bearer mF_9.B5f-4.1JqM">>)
      end},
     {"hd.www_authenticate", fun() ->
          cow_http_hd:parse_www_authenticate(<<"Basic realm=\"Nera\", charset=\"UTF-8\"">>)
      end},
     {"hd.forwarded", fun() ->
          cow_http_hd:parse_x_forwarded_for(<<"192.0.2.43, 2001:db8:cafe::17">>)
      end},
     {"hd.upgrade", fun() -> cow_http_hd:parse_upgrade(<<"websocket">>) end},
     {"hd.sec_websocket_extensions", fun() ->
          cow_http_hd:parse_sec_websocket_extensions(
            <<"permessage-deflate; client_max_window_bits">>)
      end},
     {"hd.access_control_allow_methods", fun() ->
          bin(cow_http_hd:access_control_allow_methods([<<"GET">>, <<"POST">>]))
      end},

     %% ---- cow_http_struct_hd: structured fields (RFC 8941) ----------
     {"struct.parse_dictionary", fun() ->
          cow_http_struct_hd:parse_dictionary(<<"a=1, b=?0, c=(1 2);x=\"y\", d=:aGk=:">>)
      end},
     {"struct.parse_list", fun() ->
          cow_http_struct_hd:parse_list(<<"sugar, tea, rum;q=0.5">>)
      end},
     {"struct.roundtrip_dictionary", fun() ->
          bin(cow_http_struct_hd:dictionary(
                cow_http_struct_hd:parse_dictionary(<<"u=3, i">>)))
      end},

     %% ---- cow_http / cow_http1: HTTP/1.1 messages --------------------
     {"http1.parse_request_line", fun() ->
          cow_http1:parse_request_line(<<"GET /path?x=1 HTTP/1.1\r\nHost: a\r\n">>)
      end},
     {"http1.parse_status_line", fun() ->
          cow_http1:parse_status_line(<<"HTTP/1.1 404 Not Found\r\nrest">>)
      end},
     {"http1.parse_headers", fun() ->
          cow_http1:parse_headers(<<"Host: example.org\r\nContent-Length: 5\r\n\r\nhello">>)
      end},
     {"http1.request", fun() ->
          bin(cow_http1:request(<<"POST">>, <<"/submit">>, 'HTTP/1.1',
                                [{<<"host">>, <<"example.org">>},
                                 {<<"content-length">>, <<"2">>}]))
      end},
     {"http1.response", fun() ->
          bin(cow_http1:response(200, 'HTTP/1.1', [{<<"server">>, <<"nera">>}]))
      end},
     {"http.parse_fullpath", fun() -> cow_http:parse_fullpath(<<"/a/b?c=d">>) end},
     {"http.merge_pseudo_headers", fun() ->
          cow_http:merge_pseudo_headers(#{method => <<"GET">>, scheme => <<"https">>,
                                          authority => <<"example.org">>, path => <<"/">>},
                                        [{<<"accept">>, <<"*/*">>}])
      end},
     {"http.process_headers", fun() ->
          cow_http:process_headers([{<<":method">>, <<"POST">>}, {<<":scheme">>, <<"https">>},
                                    {<<":authority">>, <<"example.org">>},
                                    {<<":path">>, <<"/form">>},
                                    {<<"content-length">>, <<"3">>}],
                                   request, undefined, nofin, #{})
      end},

     %% ---- cow_http_te: transfer-encoding ----------------------------
     {"te.chunk", fun() -> bin(cow_http_te:chunk(<<"hello world">>)) end},
     {"te.last_chunk", fun() -> cow_http_te:last_chunk() end},
     {"te.stream_chunked", fun() ->
          cow_http_te:stream_chunked(<<"5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n">>, {0, 0})
      end},
     {"te.stream_identity", fun() -> cow_http_te:stream_identity(<<"abc">>, {0, 10}) end},

     %% ---- cow_multipart ---------------------------------------------
     {"multipart.build_and_parse", fun() ->
          B = <<"frontier">>,
          Body = bin([cow_multipart:first_part(B, [{<<"content-disposition">>,
                                                    <<"form-data; name=\"field\"">>}]),
                      <<"value">>,
                      cow_multipart:close(B)]),
          {ok, Headers, Rest} = cow_multipart:parse_headers(Body, B),
          {done, Value, _Epilogue} = cow_multipart:parse_body(Rest, B),
          {Headers, Value, cow_multipart:form_data(Headers)}
      end},
     {"multipart.content_disposition", fun() ->
          cow_multipart:parse_content_disposition(
            <<"form-data; name=\"file\"; filename=\"a.txt\"">>)
      end},
     {"multipart.boundary_size", fun() -> byte_size(cow_multipart:boundary()) end},

     %% ---- cow_mimetypes ---------------------------------------------
     {"mimetypes.web_html", fun() -> cow_mimetypes:web(<<"index.html">>) end},
     {"mimetypes.all_png", fun() -> cow_mimetypes:all(<<"photo.PNG">>) end},
     {"mimetypes.web_unknown", fun() -> cow_mimetypes:web(<<"file.unknownext">>) end},

     %% ---- cow_link ----------------------------------------------------
     {"link.parse", fun() ->
          cow_link:parse_link(<<"</style.css>; rel=preload; as=style, <https://x.org/>; rel=\"next\"">>)
      end},
     {"link.build", fun() ->
          bin(cow_link:link([#{target => <<"/a">>, rel => <<"next">>, attributes => []}]))
      end},

     %% ---- cow_uri_template (RFC 6570) -------------------------------
     {"uri_template.expand", fun() ->
          bin(cow_uri_template:expand(<<"/users/{id}{?fields*}">>,
                                      #{<<"id">> => 42,
                                        <<"fields">> => [<<"name">>, <<"email">>]}))
      end},

     %% ---- cow_sse: server-sent events --------------------------------
     {"sse.parse", fun() ->
          cow_sse:parse(<<"event: tick\ndata: hello\nid: 7\n\n">>, cow_sse:init())
      end},
     {"sse.events", fun() ->
          bin(cow_sse:events([#{id => <<"1">>, event => <<"tick">>, data => <<"a\nb">>}]))
      end},

     %% ---- cow_iolists ---------------------------------------------------
     {"iolists.split", fun() ->
          {A, B} = cow_iolists:split(4, [<<"ab">>, [<<"cd">>, <<"ef">>], <<"gh">>]),
          {bin(A), bin(B)}
      end},

     %% ---- cow_ws: Websocket -------------------------------------------
     {"ws.encode_key", fun() -> cow_ws:encode_key(<<"dGhlIHNhbXBsZSBub25jZQ==">>) end},
     {"ws.frame_text", fun() -> bin(cow_ws:frame({text, <<"Hello">>}, #{})) end},
     {"ws.frame_close", fun() -> bin(cow_ws:frame({close, 1000, <<"bye">>}, #{})) end},
     {"ws.parse_masked", fun() ->
          %% RFC 6455 section 5.7: a masked "Hello" from a client.
          Frame = <<16#81, 16#85, 16#37, 16#fa, 16#21, 16#3d,
                    16#7f, 16#9f, 16#4d, 16#51, 16#58>>,
          {Type, FragState, Rsv, Len, MaskKey, Rest} = cow_ws:parse_header(Frame, #{}, undefined),
          {ok, Payload, Utf8, <<>>} =
              cow_ws:parse_payload(Rest, MaskKey, 0, 0, Type, Len, FragState, #{}, Rsv),
          {Type, Len, Payload, Utf8, cow_ws:make_frame(Type, Payload, undefined, FragState)}
      end},
     {"ws.negotiate_deflate", fun() ->
          {ok, Ext, Exts} = cow_ws:negotiate_permessage_deflate(
                              [<<"client_max_window_bits">>], #{}, #{}),
          {Ext, lists:sort(maps:keys(Exts))}
      end},

     %% ---- cow_deflate (zlib-backed inflate with a size limit) --------
     {"deflate.inflate", fun() ->
          Compressed = deflate_raw(<<"Nera runs Cowlib natively. Nera runs Cowlib natively.">>),
          Z = zlib:open(),
          ok = zlib:inflateInit(Z, -15),
          R = cow_deflate:inflate(Z, Compressed, 1000),
          zlib:close(Z),
          R
      end},
     {"deflate.size_limit", fun() ->
          Compressed = deflate_raw(binary:copy(<<"a">>, 5000)),
          Z = zlib:open(),
          ok = zlib:inflateInit(Z, -15),
          R = cow_deflate:inflate(Z, Compressed, 100),
          zlib:close(Z),
          R
      end},

     %% ---- cow_hpack: HTTP/2 header compression -----------------------
     {"hpack.roundtrip", fun() ->
          Headers = [{<<":method">>, <<"GET">>}, {<<":path">>, <<"/index.html">>},
                     {<<"user-agent">>, <<"cowlib-demo">>}, {<<"x-custom">>, <<"nera">>}],
          {Encoded, _} = cow_hpack:encode(Headers),
          {Decoded, _} = cow_hpack:decode(bin(Encoded)),
          {byte_size(bin(Encoded)), Decoded =:= Headers, Decoded}
      end},
     {"hpack.rfc7541_c4_1", fun() ->
          %% RFC 7541 C.4.1, first request with Huffman coding.
          {Decoded, _} = cow_hpack:decode(<<16#82, 16#86, 16#84, 16#41, 16#8c, 16#f1, 16#e3,
                                            16#c2, 16#e5, 16#f2, 16#3a, 16#6b, 16#a0, 16#ab,
                                            16#90, 16#f4, 16#ff>>),
          Decoded
      end},

     %% ---- cow_http2: frames -------------------------------------------
     {"http2.settings_roundtrip", fun() ->
          Frame = bin(cow_http2:settings(#{max_concurrent_streams => 100,
                                           initial_window_size => 65535})),
          {Frame, cow_http2:parse(Frame)}
      end},
     {"http2.ping_roundtrip", fun() -> cow_http2:parse(bin(cow_http2:ping(12345))) end},
     {"http2.data_roundtrip", fun() ->
          cow_http2:parse(bin(cow_http2:data(1, fin, <<"body">>)))
      end},
     {"http2.goaway", fun() ->
          cow_http2:parse(bin(cow_http2:goaway(7, no_error, <<"bye">>)))
      end},

     %% ---- cow_http2_machine: one GET request, client to server -------
     {"http2_machine.request", fun() -> http2_exchange() end},

     %% ---- cow_http3 / cow_qpack / cow_capsule / cow_http3_machine ----
     {"http3.int_roundtrip", fun() ->
          [cow_http3:parse_int(cow_http3:encode_int(N)) || N <- [37, 15293, 494878333]]
      end},
     {"http3.settings_frame", fun() -> bin(cow_http3:settings(#{})) end},
     {"http3.data_roundtrip", fun() -> cow_http3:parse(bin(cow_http3:data(<<"hi">>))) end},
     {"http3.error_codes", fun() ->
          [cow_http3:code_to_error(C) || C <- [16#100, 16#101, 16#10c]]
      end},
     {"qpack.roundtrip", fun() ->
          Headers = [{<<":method">>, <<"GET">>}, {<<":path">>, <<"/">>},
                     {<<"x-demo">>, <<"nera">>}],
          {ok, Data, _EncData, _} = cow_qpack:encode_field_section(Headers, 0,
                                                                    cow_qpack:init(encoder)),
          {ok, Decoded, _DecData, _} = cow_qpack:decode_field_section(bin(Data), 0,
                                                                      cow_qpack:init(decoder)),
          Decoded
      end},
     {"capsule.drain_roundtrip", fun() ->
          cow_capsule:parse(cow_capsule:wt_drain_session())
      end},
     {"http3_machine.request", fun() -> http3_exchange() end}
    ].

deflate_raw(Data) ->
    Z = zlib:open(),
    ok = zlib:deflateInit(Z, default, deflated, -15, 8, default),
    Out = bin(zlib:deflate(Z, Data, finish)),
    ok = zlib:deflateEnd(Z),
    zlib:close(Z),
    Out.

%% A client machine prepares a GET request; a server machine reads it.
http2_exchange() ->
    {ok, ClientPreface, C0} = cow_http2_machine:init(client, #{}),
    {ok, _ServerPreface, S0} = cow_http2_machine:init(server, #{}),
    %% Strip the 24-byte connection preface, then read the client SETTINGS.
    <<"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n", ClientSettings/binary>> = bin(ClientPreface),
    {ok, SettingsFrame, <<>>} = cow_http2:parse(ClientSettings),
    {ok, S1} = cow_http2_machine:frame(SettingsFrame, S0),
    {ok, StreamID, C1} = cow_http2_machine:init_stream(<<"GET">>, C0),
    {ok, IsFin, HeaderBlock, _C2} = cow_http2_machine:prepare_headers(
        StreamID, C1, fin,
        #{method => <<"GET">>, scheme => <<"https">>,
          authority => <<"example.org">>, path => <<"/hello">>},
        [{<<"accept">>, <<"text/plain">>}]),
    HeadersFrame = bin(cow_http2:headers(StreamID, IsFin, HeaderBlock)),
    {ok, ParsedHeaders, <<>>} = cow_http2:parse(HeadersFrame),
    {ok, Event, S2} = cow_http2_machine:frame(ParsedHeaders, S1),
    {StreamID, IsFin, Event,
     cow_http2_machine:get_stream_remote_state(StreamID, S2),
     cow_http2_machine:get_last_streamid(S2)}.

%% A client HTTP/3 machine prepares headers; a server machine reads them.
http3_exchange() ->
    {ok, _Settings, C0} = cow_http3_machine:init(client, #{}),
    {ok, _Settings2, S0} = cow_http3_machine:init(server, #{}),
    C1 = cow_http3_machine:init_bidi_stream(0, <<"GET">>, C0),
    {ok, IsFin, HeaderBlock, _Instrs, _C2} = cow_http3_machine:prepare_headers(
        0, C1, fin,
        #{method => <<"GET">>, scheme => <<"https">>,
          authority => <<"example.org">>, path => <<"/h3">>},
        [{<<"accept">>, <<"*/*">>}]),
    S1 = cow_http3_machine:init_bidi_stream(0, S0),
    {ok, Frame, <<>>} = cow_http3:parse(bin(cow_http3:headers(HeaderBlock))),
    Result = cow_http3_machine:frame(Frame, IsFin, 0, S1),
    {IsFin, element(2, Result)}.
