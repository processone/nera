# Test Plan: OTP & Armstrong-Inspired Tests

Tests inspired from Erlang/OTP compiler test suites and Joe Armstrong's
"Programming Erlang" (2nd ed). Ordered by priority.

## Status Legend
- [ ] Not started
- [x] Done
- [~] Partial (some subtests skipped due to known bugs)

## Tests

- [x] **1. guard_binary** — Guards on binary patterns (`f(<<X:8>>) when X > 10`), `byte_size` in guards, combined type+value guards.
- [x] **2. nested_receive** — Receive inside receive, receive inside spawned process callback, nested selective receive with timeout 0 (non-blocking peek).
- [x] **3. nested_try_catch** — Try inside catch inside try, exception in after block, throw vs error vs exit semantics, re-raise patterns. (Tuple-in-catch-body parser bug worked around — see KNOWN_BUGS.md #1)
- [x] **4. link_cascade** — Link chain exit propagation, on-exit restart, normal exit trapping.
- [x] **5. binary_append_loop** — Build a binary incrementally in a loop (`<<Acc/binary, Byte>>`), verify correctness at 100, 1000 bytes, chunk append, round-trip.
- [x] **6. selective_receive_stress** — Send 500 tagged messages, receive in reverse order, interleaved tags received by type, selective skip over 100 noise messages.
- [x] **7. fun_equality_ets** — Fun self-equality, closures with different envs, funs in tuples/lists/maps, funs stored/retrieved from ETS, `is_function/1`. (is_function/2 not supported — see KNOWN_BUGS.md #5)
- [x] **8. ip_packet_parse** — IPv4 header construction and parsing with sub-byte fields (4-bit version/IHL, 3-bit flags, 13-bit fragment offset), nibble pack/unpack.
- [x] **9. guard_multi_bif** — Guards with multiple BIFs (`is_list, length, hd`), `andalso`/`orelse`, guard sequences (`;`), `abs` in guards, type combos, division safety.
- [x] **10. pmap_crash** — Parallel map with index-tagged results, crash detection via monitor, larger pmap (20 elements). (Selective-receive-by-PID limitation worked around — see KNOWN_BUGS.md #4)
