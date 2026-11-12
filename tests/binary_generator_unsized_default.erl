%% EXPECT_COMPILE_ERROR: binary fields without size are not allowed in patterns of bit string generators
%% NERA-103 F1: OTP rejects the default even though no code creates the record.
-module(binary_generator_unsized_default).
-export([test/0]).
-record(chunks, {bytes = [X || <<X/binary>> <= <<>>]}).
test() -> ok.
