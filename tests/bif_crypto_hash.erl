%% EXPECT: bif_crypto_hash_ok
%% Regression: crypto:hash/2 and crypto:strong_rand_bytes/1 are available and
%% match OTP 29. Before the fix, nera check and nera build rejected Cowlib's
%% cow_ws (crypto:hash(sha, [Key, Guid])) and cow_http_hd
%% (crypto:strong_rand_bytes(48)) as unresolved functions, and calls through
%% apply raised undef. Digests are published test vectors, so this one
%% fixture runs natively and on BEAM with identical output. Reasons are
%% reduced to their first element: BEAM's unknown-type reason is
%% {badarg, {File, Line}, Text}, native raises plain badarg.
-module(bif_crypto_hash).
-export([test/0]).

test() ->
    %% Published vectors for "abc" and "" (RFC 1321, FIPS 180).
    <<16#90, 16#01, 16#50, 16#98, 16#3c, 16#d2, 16#4f, 16#b0,
      16#d6, 16#96, 16#3f, 16#7d, 16#28, 16#e1, 16#7f, 16#72>> =
        crypto:hash(md5, id(<<"abc">>)),
    <<"a9993e364706816aba3e25717850c26c9cd0d89d">> = hex(crypto:hash(sha, id(<<"abc">>))),
    <<"da39a3ee5e6b4b0d3255bfef95601890afd80709">> = hex(crypto:hash(sha, id(<<>>))),
    <<"23097d223405d8228642a477bda255b32aadbce4bda0b3f7e36c9da7">> =
        hex(crypto:hash(sha224, id(<<"abc">>))),
    <<"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad">> =
        hex(crypto:hash(sha256, id(<<"abc">>))),
    <<"cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed",
      "8086072ba1e7cc2358baeca134c825a7">> = hex(crypto:hash(sha384, id(<<"abc">>))),
    <<"ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a",
      "2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f">> =
        hex(crypto:hash(sha512, id(<<"abc">>))),
    16 = byte_size(crypto:hash(md5, <<>>)),
    20 = byte_size(crypto:hash(sha, <<>>)),

    %% Iodata: nested lists, strings, binary tails; Cowlib hashes a pair.
    Expected = crypto:hash(sha, <<"abcd">>),
    Expected = crypto:hash(sha, id([<<"a">>, "b", $c | <<"d">>])),
    Expected = crypto:hash(sha, id([[<<"ab">>], [[$c], []], <<"d">>])),
    Key = <<"dGhlIHNhbXBsZSBub25jZQ==">>,
    %% RFC 6455 section 1.3 handshake example.
    <<"s3pPLMBiTxaQ9kYGzzhZRbK+xOo=">> =
        base64:encode(crypto:hash(sha, [Key, "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"])),

    %% A large input and a sub-binary of a larger one.
    Big = id(binary:copy(<<"a">>, 1000000)),
    <<"34aa973cd4c4daa4f61eeb2bdbad27316534016f">> = hex(crypto:hash(sha, Big)),
    <<_:8, Sub/binary>> = id(<<"xabc">>),
    true = crypto:hash(sha, <<"abc">>) =:= crypto:hash(sha, Sub),

    %% Random bytes: length, type, and not constant.
    <<>> = crypto:strong_rand_bytes(id(0)),
    1 = byte_size(crypto:strong_rand_bytes(id(1))),
    48 = byte_size(crypto:strong_rand_bytes(id(48))),
    100000 = byte_size(crypto:strong_rand_bytes(id(100000))),
    true = is_binary(crypto:strong_rand_bytes(id(16))),
    true = crypto:strong_rand_bytes(id(32)) =/= crypto:strong_rand_bytes(id(32)),
    true = lists:usort([crypto:strong_rand_bytes(id(1)) || _ <- lists:seq(1, 200)]) =/= [<<0>>],
    true = length(lists:usort([crypto:strong_rand_bytes(id(1)) || _ <- lists:seq(1, 2000)])) > 200,

    %% Dynamic calls, erlang-free module names, apply and fun references.
    Mod = id(crypto),
    HFun = id(hash),
    Expected = apply(Mod, HFun, [sha, <<"abcd">>]),
    Expected = Mod:HFun(sha, <<"abcd">>),
    Expected = (fun crypto:hash/2)(sha, <<"abcd">>),
    Expected = (fun Mod:HFun/2)(sha, <<"abcd">>),
    32 = byte_size(apply(Mod, id(strong_rand_bytes), [32])),
    32 = byte_size((fun crypto:strong_rand_bytes/1)(32)),
    32 = byte_size(Mod:(id(strong_rand_bytes))(32)),

    %% Inputs OTP 29 rejects.
    badarg = reason(fun() -> crypto:hash(id(foo), <<>>) end),
    badarg = reason(fun() -> crypto:hash(id(1), <<>>) end),
    badarg = reason(fun() -> crypto:hash(id(sha), id(foo)) end),
    badarg = reason(fun() -> crypto:hash(id(sha), id(<<1:3>>)) end),
    badarg = reason(fun() -> crypto:hash(id(sha), id([a])) end),
    badarg = reason(fun() -> crypto:hash(id(sha), id([256])) end),
    badarg = reason(fun() -> apply(Mod, HFun, [foo, <<>>]) end),
    badarg = reason(fun() -> crypto:strong_rand_bytes(id(-1)) end),
    badarg = reason(fun() -> crypto:strong_rand_bytes(id(a)) end),
    badarg = reason(fun() -> crypto:strong_rand_bytes(id(1.0)) end),
    badarg = reason(fun() -> crypto:strong_rand_bytes(id(<<1>>)) end),
    badarg = reason(fun() -> apply(Mod, id(strong_rand_bytes), [-1]) end),

    io:format("bif_crypto_hash_ok~n"),
    ok.

hex(Bin) -> << <<(hexc(N)), (hexc(M))>> || <<N:4, M:4>> <= Bin >>.

hexc(N) when N < 10 -> $0 + N;
hexc(N) -> $a + N - 10.

reason(F) ->
    try F() of
        Value -> {returned, Value}
    catch
        error:{badarg, _, _} -> badarg;
        error:Reason -> Reason
    end.

id(X) -> X.
