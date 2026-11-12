#!/usr/bin/env python3
"""NERA-92: compare real import-resolution diagnostics with OTP 29.0.4.

Focused CLI check probes do not build native programs. Native completion fixtures
are tests/import_resolution*.erl and must also run on both Nera and BEAM.
"""
import argparse
from pathlib import Path
import subprocess
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--nera", required=True, type=Path)
parser.add_argument("--observe", action="store_true", help="Record baseline differences without asserting Nera results")
args = parser.parse_args()
nera = args.nera.resolve()
# name, source forms, OTP error token (None means accepted), Nera diagnostic token
cases = [
    ("explicit_import", "-import(lists,[reverse/1]). f() -> reverse([]).", None, None),
    ("imported_fun", "-import(lists,[reverse/1]). f() -> fun reverse/1.", "fun_import", "imported"),
    ("import_local_conflict", "-import(lists,[reverse/1]). reverse(X) -> X. f() -> reverse([]).", "define_import", "conflicting import"),
    ("selective", "-compile({no_auto_import,[length/1]}). f() -> length([]).", "undefined_function", "undefined"),
    ("ignored_suppression_entries", "-compile({no_auto_import,[ignored,42,{length,bad},{7,1},{length,1,extra},#{x=>1},{length,4294967297},{length,-1}]}). f() -> length([]).", None, None),
    ("mixed_suppression_entries", "-compile({no_auto_import,[ignored,42,{length,bad},[self/0]]}). f() -> self().", "undefined_function", "undefined"),
    ("option_list", "-compile([debug_info,{no_auto_import,[length/1]}]). f() -> length([]).", "undefined_function", "undefined"),
    ("global_self", "-compile(no_auto_import). f() -> self().", "undefined_function", "undefined"),
    ("global_throw", "-compile([no_auto_import]). f() -> throw(boom).", "undefined_function", "undefined"),
    ("suppressed_spawn", "-compile({no_auto_import,[spawn/3]}). f() -> spawn(p,f,[]).", "undefined_function", "undefined"),
    ("suppressed_bytes", "-compile({no_auto_import,[byte_size/1]}). f() -> byte_size(<<>>).", "undefined_function", "undefined"),
    ("suppressed_bits", "-compile({no_auto_import,[bit_size/1]}). f() -> bit_size(<<>>).", "undefined_function", "undefined"),
    ("suppressed_fun", "-compile({no_auto_import,[length/1]}). f() -> fun length/1.", "undefined_function", "undefined"),
    ("local_clash", "f() -> length([]). length(_) -> 1.", None, "ambiguous call"),
    ("local_fun_clash", "f() -> fun length/1. length(_) -> 1.", None, "ambiguous call"),
    ("import_clash", "-import(other,[float/1]). f() -> float(1).", None, "import overrides"),
    ("guard_shadow", "f() -> case [] of X when length(X)>0 -> yes; _ -> no end. length(_) -> 1.", "illegal_guard_local_call", "illegal in guard"),
    ("guard_suppressed", "-compile(no_auto_import). f() -> case [] of X when length(X)>0 -> yes; _ -> no end.", "illegal_guard_expr", "illegal in guard"),
    ("guard_reimport", "-compile(no_auto_import). -import(erlang,[length/1]). f() -> case [] of X when length(X)>0 -> yes; _ -> no end.", None, None),
    ("extension", "f() -> phash2(a).", "undefined_function", "undefined"),
    ("qualified_extension", "f() -> erlang:phash2(a).", None, None),
    ("record_form", "-record(r,{x}). f() -> is_record({},r).", None, "compiler-specific"),
    ("record_guard", "-record(r,{x}). f() -> case {} of X when is_record(X,r) -> yes; _ -> no end.", None, "compiler-specific"),
    ("qualified_record_guard", "-record(r,{x}). f() -> case {} of X when erlang:is_record(X,r,2) -> yes; _ -> no end.", None, "compiler-specific"),
    ("nested_record_guard", "-record(r,{x}). f() -> (fun(X) when is_record(X,r) -> yes; (_) -> no end)({}).", None, "compiler-specific"),
    ("merged_get_keys", "f() -> get_keys(active).", None, None),
    ("missing_native", "f() -> alias().", None, "erlang:alias/0"),
]
with tempfile.TemporaryDirectory(prefix="nera-import-resolution-") as work:
    work = Path(work)
    for name, forms, otp_error, nera_token in cases:
        folder = work / name
        folder.mkdir()
        source = folder / "p.erl"
        source.write_text("-module(p). -export([f/0]). " + forms + "\n")
        query = ('R=compile:file("' + str(source) + '",[binary,return_errors,return_warnings]),'
                 'case R of {ok,_,_,W}->io:format("accepted ~p~n",[W]);'
                 '{error,E,W}->io:format("rejected ~p ~p~n",[E,W]) end,halt().')
        otp = subprocess.run(["erl", "+S", "1", "-noshell", "-eval", query], text=True, capture_output=True, check=True).stdout
        if otp_error is None:
            assert otp.startswith("accepted"), (name, otp)
        else:
            assert otp.startswith("rejected") and otp_error in otp, (name, otp)
        result = subprocess.run([str(nera), "check", str(source)], cwd=folder, text=True, capture_output=True)
        text = result.stdout + result.stderr
        print(f"{name}: OTP {'rejects' if otp_error else 'accepts'}; Nera exit {result.returncode}")
        print(text.rstrip())
        if not args.observe:
            assert result.returncode in (0, 1), (name, result.returncode, text)
            if nera_token:
                assert nera_token in text, (name, text)
            else:
                assert result.returncode == 0, (name, text)
            if name in ("record_form", "record_guard", "qualified_record_guard", "nested_record_guard"):
                assert result.returncode == 1, (name, text)
            if name in ("local_clash", "local_fun_clash"):
                assert "call_to_redefined_bif" in otp and result.returncode == 0
            if name == "import_clash":
                assert "redefine_bif_import" in otp
    print(f"Completed {len(cases)} OTP/Nera semantic probes")
