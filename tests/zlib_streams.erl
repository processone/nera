%% EXPECT: zlib_streams_ok
%% Regression: the zlib calls made by cowlib's cow_ws and cow_deflate
%% (open/0, deflateInit/6, deflate/3, deflateReset/1, inflateInit/2,
%% inflateReset/1, safeInflate/2, set_controlling_process/2) failed to build
%% on Nera with "module zlib is not available". Streams must behave as on
%% OTP: a raw deflate context sync-flushed per message, inflated by a peer
%% context, the sync-flush marker, no_takeover resets, safeInflate output
%% limits, and OTP's error reasons. Compressed bytes are compared only through
%% round trips, so the fixture holds for any conforming deflate.
%% Run this same fixture on native Nera and BEAM.
-module(zlib_streams).
-export([test/0]).

test() ->
    ok = context_takeover(),
    ok = no_takeover(),
    ok = safe_inflate_limit(),
    ok = iodata_and_flush_modes(),
    ok = errors(),
    ok = dynamic_calls(),
    io:format("zlib_streams_ok~n"),
    ok.

%% cow_ws hands its inflate context to the connection process. After the
%% hand-over only the new controller may use it.
hand_over(Z) ->
    Self = self(),
    Pid = spawn(fun() ->
        receive
            {take, Ref} ->
                ok = zlib:inflateInit(Z, -15),
                Self ! {Ref, ok},
                receive {give_back, Ref2, To} ->
                    ok = zlib:set_controlling_process(Z, To),
                    To ! {Ref2, ok}
                end
        end
    end),
    Ref = make_ref(),
    ok = zlib:set_controlling_process(Z, Pid),
    {not_on_controlling_process, _} = reason(fun() -> zlib:inflateInit(Z, -15) end),
    {not_on_controlling_process, _} = reason(fun() -> zlib:set_controlling_process(Z, self()) end),
    Pid ! {take, Ref},
    receive {Ref, ok} -> ok end,
    Ref2 = make_ref(),
    Pid ! {give_back, Ref2, self()},
    receive {Ref2, ok} -> ok end,
    ok.

deflater() ->
    Z = zlib:open(),
    ok = zlib:deflateInit(Z, best_compression, deflated, -15, 8, default),
    Z.

inflater() ->
    Z = zlib:open(),
    ok = zlib:inflateInit(Z, -15),
    Z.

inflate_all(Z, Data) ->
    inflate_all(zlib:safeInflate(Z, Data), Z, []).

inflate_all({continue, Out}, Z, Acc) ->
    inflate_all(zlib:safeInflate(Z, []), Z, [Acc, Out]);
inflate_all({finished, Out}, _Z, Acc) ->
    iolist_to_binary([Acc, Out]).

%% cow_ws permessage-deflate with context takeover.
context_takeover() ->
    D = deflater(),
    I = zlib:open(),
    ok = hand_over(I),
    Msg1 = <<"hello hello hello hello hello">>,
    Msg2 = <<"hello hello hello hello world">>,
    P1 = iolist_to_binary(zlib:deflate(D, Msg1, sync)),
    P2 = iolist_to_binary(zlib:deflate(D, Msg2, sync)),
    %% Sync flush ends every message with an empty stored block.
    true = ends_with_sync_marker(P1),
    true = ends_with_sync_marker(P2),
    Msg1 = inflate_all(I, P1),
    Msg2 = inflate_all(I, P2),
    ok.

ends_with_sync_marker(Bin) ->
    Skip = byte_size(Bin) - 4,
    <<_:Skip/binary, 0, 0, 255, 255>> = Bin,
    true.

%% cow_deflate / cow_ws no_takeover: reset the contexts after each message.
no_takeover() ->
    D = deflater(),
    I = inflater(),
    lists:foreach(
        fun(Msg) ->
            P = iolist_to_binary(zlib:deflate(D, Msg, sync)),
            ok = zlib:deflateReset(D),
            Msg = inflate_all(I, P),
            ok = zlib:inflateReset(I)
        end,
        [<<"first message">>, <<"second message">>, binary:copy(<<"abc">>, 500)]),
    ok.

%% safeInflate returns {continue, _} while a chunk limit stops expansion.
safe_inflate_limit() ->
    D = deflater(),
    Big = binary:copy(<<"a">>, 200000),
    P = iolist_to_binary(zlib:deflate(D, Big, finish)),
    I = inflater(),
    {continue, First} = zlib:safeInflate(I, P),
    true = iolist_size(First) < byte_size(Big),
    Rest = inflate_all(I, []),
    Big = iolist_to_binary([First, Rest]),
    ok = zlib:inflateEnd(I),
    ok.

iodata_and_flush_modes() ->
    D = deflater(),
    I = inflater(),
    Packed = iolist_to_binary(
        [zlib:deflate(D, [<<"ab">>, "cd", [<<"ef">>]], none),
         zlib:deflate(D, <<"gh">>, full)]),
    <<"abcdefgh">> = inflate_all(I, Packed),
    Z = zlib:open(),
    ok = zlib:deflateInit(Z, 6),
    Whole = iolist_to_binary([zlib:deflate(Z, <<"whole stream">>, finish)]),
    Z2 = zlib:open(),
    ok = zlib:inflateInit(Z2),
    <<"whole stream">> = iolist_to_binary(zlib:inflate(Z2, Whole)),
    ok = zlib:inflateEnd(Z2),
    ok.

errors() ->
    Z = zlib:open(),
    {not_initialized, _} = reason(fun() -> zlib:deflate(Z, <<"a">>, sync) end),
    {not_initialized, _} = reason(fun() -> zlib:safeInflate(Z, <<"a">>) end),
    {not_initialized, _} = reason(fun() -> zlib:deflateReset(Z) end),
    {not_initialized, _} = reason(fun() -> zlib:inflateReset(Z) end),
    {bad_windowbits, _} = reason(fun() -> zlib:inflateInit(Z, 99) end),
    {bad_compression_level, _} = reason(fun() -> zlib:deflateInit(Z, 12) end),
    {bad_compression_method, _} =
        reason(fun() -> zlib:deflateInit(Z, 6, other, 15, 8, default) end),
    {bad_compression_strategy, _} =
        reason(fun() -> zlib:deflateInit(Z, 6, deflated, 15, 8, other) end),
    {bad_memlevel, _} =
        reason(fun() -> zlib:deflateInit(Z, 6, deflated, 15, 10, default) end),
    ok = zlib:deflateInit(Z, 6, deflated, -15, 8, default),
    {already_initialized, _} = reason(fun() -> zlib:inflateInit(Z, -15) end),
    {already_initialized, _} = reason(fun() -> zlib:deflateInit(Z, 6) end),
    {not_initialized, _} = reason(fun() -> zlib:safeInflate(Z, <<"a">>) end),
    {bad_flush_mode, _} = reason(fun() -> zlib:deflate(Z, <<"a">>, other) end),
    {badarg, _} = reason(fun() -> zlib:deflate(Z, an_atom, sync) end),
    {badarg, _} = reason(fun() -> zlib:deflate(make_ref(), <<"a">>, sync) end),
    {badarg, _} = reason(fun() -> zlib:deflate(not_a_ref, <<"a">>, sync) end),
    {badarg, _} = reason(fun() -> zlib:set_controlling_process(Z, not_a_pid) end),
    ok = zlib:set_controlling_process(Z, self()),
    I = inflater(),
    {data_error, _} = reason(fun() -> zlib:safeInflate(I, <<"garbage garbage">>) end),
    %% A finished stream rejects more compression until reset.
    F = deflater(),
    _ = zlib:deflate(F, <<"abc">>, finish),
    {stream_error, _} = reason(fun() -> zlib:deflate(F, <<"more">>, sync) end),
    ok = zlib:deflateReset(F),
    _ = zlib:deflate(F, <<"more">>, sync),
    ok = zlib:close(Z),
    {not_initialized, _} = reason(fun() -> zlib:close(Z) end),
    ok.

%% Same calls through apply and variable module/function names.
dynamic_calls() ->
    M = id(zlib),
    Z = M:open(),
    ok = apply(M, deflateInit, [Z, best_compression, deflated, -15, 8, default]),
    Packed = iolist_to_binary(apply(zlib, deflate, [Z, <<"dynamic dynamic">>, sync])),
    F = id(fun zlib:inflateInit/2),
    I = zlib:open(),
    ok = F(I, -15),
    <<"dynamic dynamic">> = inflate_all(I, Packed),
    true = is_reference(Z),
    ok.

reason(F) ->
    try F() of
        V -> {no_error, V}
    catch
        error:R -> {R, error}
    end.

id(X) -> X.
