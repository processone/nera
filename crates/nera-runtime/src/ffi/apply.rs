//! Dynamic function dispatch (`erlang:apply/2`, `erlang:apply/3`).

#![allow(unused_unsafe)]

use crate::atom_table::global_atom_table;
use crate::entry::global_function_table;
use crate::process::ProcessContext;
use crate::term::ErlTerm;
use super::helpers::*;

// Import extern fns from sibling modules for BIF dispatch
use super::closure::*;
use super::unicode::*;
use super::list_ops::*;
use super::type_check::*;
use super::map_ops::*;
use super::crypto_ops::*;
use super::binary_ops::*;
use super::base64::*;
use super::base64_list::*;
use super::string_ops::*;
use super::io_format::*;
use super::exception::*;
use super::convert::*;
use super::tuple_ops::*;
use super::misc::*;
use super::process::*;
use super::ets::*;
use super::persistent_term::*;
use super::counters::*;
use super::atomics::*;
use super::zlib::*;
use super::gen_tcp::*;
use super::timer::*;

/// Dynamic function application: look up a compiled function by
/// `(module, function, arity)` in the global function table and call it.
///
/// Arguments:
/// - `ctx`: the current process context
/// - `mod_atom`: tagged atom ErlTerm for the module name
/// - `fun_atom`: tagged atom ErlTerm for the function name
/// - `args`: tagged Erlang list of arguments (used to determine arity)
///
/// Returns the result of the called function, or nil if not found.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_apply(
    ctx: *mut ProcessContext,
    mod_atom: u64,
    fun_atom: u64,
    args: u64,
) -> u64 {
    apply_impl(ctx, mod_atom, fun_atom, args, true)
}

/// `resolve` is false only for dispatch owned by the tail-call loop.
pub(super) unsafe fn apply_impl(
    ctx: *mut ProcessContext,
    mod_atom: u64,
    fun_atom: u64,
    args: u64,
    resolve: bool,
) -> u64 {
    let mod_id = match ErlTerm(mod_atom).as_atom_id() {
        Some(id) => id,
        None => return raise_badarg(ctx),
    };
    let fun_id = match ErlTerm(fun_atom).as_atom_id() {
        Some(id) => id,
        None => return raise_badarg(ctx),
    };

    // Collect args from the Erlang list and count arity
    let mut arg_terms: Vec<u64> = Vec::new();
    let mut current = ErlTerm(args);
    while current.is_cons() {
        if let Some(ptr) = current.as_cons_ptr() {
            unsafe {
                arg_terms.push((*ptr).head.0);
                current = (*ptr).tail;
            }
        } else {
            break;
        }
    }
    let arity = arg_terms.len() as u32;

    // Look up the function in the global function table
    if let Some(func) = global_function_table().lookup(mod_id, fun_id, arity) {
        // SAFETY: the context and argument storage are live and exclusively
        // accessible here; no heap borrow crosses this reentry boundary.
        unsafe { func(ctx, arg_terms.as_ptr(), arity); }
        // Resolve any pending tail calls before returning to caller
        // (the caller may have a try/catch that needs to see exceptions).
        if resolve { unsafe { super::helpers::resolve_tail_calls(ctx); } }
        return if !ctx.is_null() {
            unsafe { (*ctx).result }
        } else {
            ErlTerm::nil().0
        };
    }

    // Special case: erlang:apply/3 — recursively dispatch with the inner args
    let mod_name = global_atom_table().resolve(mod_id);
    let fun_name = global_atom_table().resolve(fun_id);
    if mod_name.as_deref() == Some("erlang") && fun_name.as_deref() == Some("apply") && arity == 3 {
        // arg_terms[0] = Module atom, arg_terms[1] = Function atom, arg_terms[2] = Args list
        return apply_impl(ctx, arg_terms[0], arg_terms[1], arg_terms[2], resolve);
    }

    // Special case: erlang:apply/2 — call a Fun with a list of arguments.
    // arg_terms[0] = Fun (closure or remote fun ref), arg_terms[1] = Args list.
    if mod_name.as_deref() == Some("erlang") && fun_name.as_deref() == Some("apply") && arity == 2 {
        let fun_term = arg_terms[0];
        let mut call_args: Vec<u64> = Vec::new();
        let mut current = ErlTerm(arg_terms[1]);
        while current.is_cons() {
            if let Some(ptr) = current.as_cons_ptr() {
                unsafe {
                    call_args.push((*ptr).head.0);
                    current = (*ptr).tail;
                }
            } else {
                break;
            }
        }
        let call_arity = call_args.len() as u32;
        return unsafe {
            super::closure::call_closure_impl(
                ctx,
                fun_term,
                call_args.as_ptr(),
                call_arity,
                resolve,
            )
        };
    }

    // Fallback: dispatch to known BIFs
    let result = match (mod_name.as_deref(), fun_name.as_deref(), arity) {
        (Some("base64"), Some("encode_binary"), 3) => Some(nera_rt_base64_encode_binary(ctx, arg_terms[0], arg_terms[1], arg_terms[2])),
        (Some("base64"), Some("decode_binary"), 3) => Some(nera_rt_base64_decode_binary(ctx, arg_terms[0], arg_terms[1], arg_terms[2])),
        (Some("base64"), Some("decode_list"), 3) => Some(nera_rt_base64_decode_list(ctx, arg_terms[0], arg_terms[1], arg_terms[2])),
        (Some("erlang"), Some("length"), 1) => Some(nera_rt_length(ctx, arg_terms[0])),
        (Some("erlang"), Some("hd"), 1) => Some(nera_rt_hd(ctx, arg_terms[0])),
        (Some("erlang"), Some("tl"), 1) => Some(nera_rt_tl(ctx, arg_terms[0])),
        (Some("erlang"), Some("abs"), 1) => Some(nera_rt_abs(ctx, arg_terms[0])),
        (Some("math"), Some("sqrt"), 1) => Some(super::arith::nera_rt_math_sqrt(ctx, arg_terms[0])),
        (Some("math"), Some("exp"), 1) => Some(super::arith::nera_rt_math_exp(ctx, arg_terms[0])),
        (Some("math"), Some("log"), 1) => Some(super::arith::nera_rt_math_log(ctx, arg_terms[0])),
        (Some("math"), Some("pow"), 2) => Some(super::arith::nera_rt_math_pow(ctx, arg_terms[0], arg_terms[1])),
        (Some("erlang"), Some("round"), 1) => Some(nera_rt_round(ctx, arg_terms[0])),
        (Some("erlang"), Some("trunc"), 1) => Some(nera_rt_trunc(ctx, arg_terms[0])),
        (Some("erlang"), Some("float"), 1) => Some(nera_rt_float(ctx, arg_terms[0])),
        (Some("erlang"), Some("ceil"), 1) => Some(nera_rt_ceil(ctx, arg_terms[0])),
        (Some("erlang"), Some("floor"), 1) => Some(nera_rt_floor(ctx, arg_terms[0])),
        (Some("erlang"), Some("element"), 2) => Some(nera_rt_element(ctx, arg_terms[0], arg_terms[1])),
        (Some("erlang"), Some("tuple_size"), 1) => Some(nera_rt_tuple_size(ctx, arg_terms[0])),
        (Some("erlang"), Some("size"), 1) => Some(nera_rt_size(ctx, arg_terms[0])),
        (Some("erlang"), Some("phash2"), 1) => Some(nera_rt_phash2(ctx, arg_terms[0])),
        (Some("erlang"), Some("phash2"), 2) => Some(nera_rt_phash2_2(ctx, arg_terms[0], arg_terms[1])),
        (Some("erlang"), Some("is_integer"), 1) => Some(nera_rt_is_integer(ctx, arg_terms[0])),
        (Some("erlang"), Some("is_atom"), 1) => Some(nera_rt_is_atom(ctx, arg_terms[0])),
        (Some("erlang"), Some("is_list"), 1) => Some(nera_rt_is_list(ctx, arg_terms[0])),
        (Some("erlang"), Some("is_tuple"), 1) => Some(nera_rt_is_tuple(ctx, arg_terms[0])),
        (Some("erlang"), Some("is_pid"), 1) => Some(nera_rt_is_pid(ctx, arg_terms[0])),
        (Some("erlang"), Some("is_binary"), 1) => Some(nera_rt_is_binary(ctx, arg_terms[0])),
        (Some("erlang"), Some("is_map"), 1) => Some(nera_rt_is_map(ctx, arg_terms[0])),
        (Some("erlang"), Some("is_function"), 1) => Some(nera_rt_is_function(ctx, arg_terms[0])),
        (Some("erlang"), Some("is_function"), 2) => Some(nera_rt_is_function2(ctx, arg_terms[0], arg_terms[1])),
        (Some("erlang"), Some("is_reference"), 1) => Some(nera_rt_is_reference(ctx, arg_terms[0])),
        (Some("erlang"), Some("make_ref"), 0) => Some(nera_rt_make_ref(ctx)),
        (Some("erlang"), Some("node"), 0) => Some(nera_rt_node(ctx)),
        (Some("erlang"), Some("node"), 1) => Some(nera_rt_node_1(ctx, arg_terms[0])),
        (Some("erlang"), Some("get_stacktrace"), 0) => Some(nera_rt_get_stacktrace(ctx)),
        (Some("erlang"), Some("is_float"), 1) => Some(nera_rt_is_float(ctx, arg_terms[0])),
        (Some("erlang"), Some("is_number"), 1) => Some(nera_rt_is_number(ctx, arg_terms[0])),
        (Some("erlang"), Some("is_boolean"), 1) => Some(nera_rt_is_boolean(ctx, arg_terms[0])),
        (Some("erlang"), Some("byte_size"), 1) => Some(nera_rt_binary_byte_size(ctx, arg_terms[0])),
        (Some("erlang"), Some("map_size"), 1) => Some(nera_rt_map_size(ctx, arg_terms[0])),
        (Some("erlang"), Some("max"), 2) => Some(nera_rt_max(ctx, arg_terms[0], arg_terms[1])),
        (Some("erlang"), Some("min"), 2) => Some(nera_rt_min(ctx, arg_terms[0], arg_terms[1])),
        (Some("erlang"), Some("atom_to_list"), 1) => Some(nera_rt_atom_to_list(ctx, arg_terms[0])),
        (Some("erlang"), Some("list_to_atom"), 1) => Some(nera_rt_list_to_atom(ctx, arg_terms[0])),
        (Some("erlang"), Some("integer_to_list"), 1) => Some(nera_rt_integer_to_list(ctx, arg_terms[0])),
        (Some("erlang"), Some("list_to_integer"), 2) => Some(nera_rt_list_to_integer_2(ctx, arg_terms[0], arg_terms[1])),
        (Some("erlang"), Some("list_to_integer"), 1) => Some(nera_rt_list_to_integer(ctx, arg_terms[0])),
        (Some("erlang"), Some("float_to_list"), 1) => Some(nera_rt_float_to_list(ctx, arg_terms[0])),
        (Some("erlang"), Some("list_to_tuple"), 1) => Some(nera_rt_list_to_tuple(ctx, arg_terms[0])),
        (Some("erlang"), Some("tuple_to_list"), 1) => Some(nera_rt_tuple_to_list(ctx, arg_terms[0])),
        (Some("erlang"), Some("integer_to_list"), 2) => Some(nera_rt_integer_to_list_2(ctx, arg_terms[0], arg_terms[1])),
        (Some("erlang"), Some("integer_to_binary"), 1) => Some(nera_rt_integer_to_binary(ctx, arg_terms[0])),
        (Some("erlang"), Some("integer_to_binary"), 2) => Some(nera_rt_integer_to_binary_2(ctx, arg_terms[0], arg_terms[1])),
        (Some("erlang"), Some("binary_to_integer"), 2) => Some(nera_rt_binary_to_integer_2(ctx, arg_terms[0], arg_terms[1])),
        (Some("erlang"), Some("binary_to_integer"), 1) => Some(nera_rt_binary_to_integer(ctx, arg_terms[0])),
        (Some("erlang"), Some("binary_to_float"), 1) => Some(nera_rt_binary_to_float(ctx, arg_terms[0])),
        (Some("erlang"), Some("list_to_binary"), 1) => Some(nera_rt_list_to_binary(ctx, arg_terms[0])),
        (Some("erlang"), Some("bitstring_to_list"), 1) => Some(nera_rt_bitstring_to_list(ctx, arg_terms[0])),
        (Some("erlang"), Some("list_to_bitstring"), 1) => Some(nera_rt_list_to_bitstring(ctx, arg_terms[0])),
        (Some("crypto"), Some("hash"), 2) => Some(nera_rt_crypto_hash(ctx, arg_terms[0], arg_terms[1])),
        (Some("crypto"), Some("strong_rand_bytes"), 1) => Some(nera_rt_crypto_strong_rand_bytes(ctx, arg_terms[0])),
        (Some("erlang"), Some("binary_to_list"), 1) => Some(nera_rt_binary_to_list(ctx, arg_terms[0])),
        (Some("erlang"), Some("iolist_size"), 1) => Some(nera_rt_iolist_size(ctx, arg_terms[0])),
        (Some("erlang"), Some("iolist_to_binary"), 1) => Some(nera_rt_iolist_to_binary(ctx, arg_terms[0])),
        (Some("erlang"), Some("atom_to_binary"), 1) => Some(nera_rt_atom_to_binary(ctx, arg_terms[0])),
        (Some("erlang"), Some("atom_to_binary"), 2) => Some(nera_rt_atom_to_binary_2(ctx, arg_terms[0], arg_terms[1])),
        (Some("erlang"), Some("float_to_binary"), 1) => Some(nera_rt_float_to_binary(ctx, arg_terms[0])),
        (Some("erlang"), Some("float_to_binary"), 2) => Some(nera_rt_float_to_binary_2(ctx, arg_terms[0], arg_terms[1])),
        (Some("erlang"), Some("binary_to_atom"), 1) => Some(nera_rt_binary_to_atom(ctx, arg_terms[0])),
        (Some("erlang"), Some("binary_to_atom"), 2) => Some(nera_rt_binary_to_atom_2(ctx, arg_terms[0], arg_terms[1])),
        (Some("erlang"), Some("binary_part"), 3) => Some(nera_rt_binary_part(ctx, arg_terms[0], arg_terms[1], arg_terms[2])),
        (Some("erlang"), Some("list_to_existing_atom"), 1) => Some(nera_rt_list_to_existing_atom(ctx, arg_terms[0])),
        (Some("erlang"), Some("binary_to_existing_atom"), 1) => Some(nera_rt_binary_to_existing_atom_1(ctx, arg_terms[0])),
        (Some("erlang"), Some("binary_to_existing_atom"), 2) => Some(nera_rt_binary_to_existing_atom(ctx, arg_terms[0], arg_terms[1])),
        (Some("erlang"), Some("is_process_alive"), 1) => Some(nera_rt_is_process_alive(ctx, arg_terms[0])),
        (Some("erlang"), Some("universaltime"), 0) => Some(nera_rt_universaltime(ctx)),
        (Some("erlang"), Some("system_time"), 1) => Some(nera_rt_system_time_1(ctx, arg_terms[0])),
        (Some("erlang"), Some("send_after"), 3) => Some(nera_rt_send_after(ctx, arg_terms[0], arg_terms[1], arg_terms[2])),
        (Some("erlang"), Some("start_timer"), 3) => Some(nera_rt_start_timer(ctx, arg_terms[0], arg_terms[1], arg_terms[2])),
        (Some("erlang"), Some("cancel_timer"), 1) => Some(nera_rt_cancel_timer(ctx, arg_terms[0])),
        (Some("erlang"), Some("cancel_timer"), 2) => Some(nera_rt_cancel_timer_2(ctx, arg_terms[0], arg_terms[1])),
        (Some("erlang"), Some("exit"), 2) => Some(nera_rt_exit_2(ctx, arg_terms[0], arg_terms[1])),
        (Some("erlang"), Some("setelement"), 3) => Some(nera_rt_setelement(ctx, arg_terms[0], arg_terms[1], arg_terms[2])),
        (Some("erlang"), Some("make_tuple"), 2) => Some(nera_rt_make_tuple_2(ctx, arg_terms[0], arg_terms[1])),
        (Some("erlang"), Some("make_tuple"), 3) => Some(nera_rt_make_tuple_3(ctx, arg_terms[0], arg_terms[1], arg_terms[2])),
        (Some("erlang"), Some("append_element"), 2) => Some(nera_rt_append_element(ctx, arg_terms[0], arg_terms[1])),
        (Some("erlang"), Some("group_leader"), 0) => Some(nera_rt_group_leader(ctx)),
        (Some("erlang"), Some("group_leader"), 2) => Some(nera_rt_group_leader_2(ctx, arg_terms[0], arg_terms[1])),
        (Some("erlang"), Some("statistics"), 1) => Some(nera_rt_statistics(ctx, arg_terms[0])),
        (Some("erlang"), Some("fun_info"), 2) => Some(nera_rt_fun_info_2(ctx, arg_terms[0], arg_terms[1])),
        (Some("erlang"), Some("fun_info_mfa"), 1) => Some(nera_rt_fun_info_mfa(ctx, arg_terms[0])),
        (Some("erlang"), Some("self"), 0) => Some(nera_rt_get_self(ctx)),
        (Some("erlang"), Some("throw"), 1) => Some(nera_rt_throw(ctx, arg_terms[0])),
        (Some("erlang"), Some("error"), 1) => Some(nera_rt_error(ctx, arg_terms[0])),
        (Some("erlang"), Some("exit"), 1) => Some(nera_rt_exit(ctx, arg_terms[0])),
        (Some("erlang"), Some("halt"), 0) => Some(nera_rt_halt_0(ctx)),
        (Some("erlang"), Some("halt"), 1) => Some(nera_rt_halt_1(ctx, arg_terms[0])),
        (Some("maps"), Some("new"), 0) => Some(nera_rt_map_new(ctx)),
        (Some("maps"), Some("get"), 2) | (Some("erlang"), Some("map_get"), 2) =>
            Some(nera_rt_map_get(ctx, arg_terms[1], arg_terms[0])),
        (Some("erlang"), Some("garbage_collect"), 0) => Some(nera_rt_garbage_collect(ctx)),
        (Some("maps"), Some("get"), 3) => Some(nera_rt_map_get_3(ctx, arg_terms[1], arg_terms[0], arg_terms[2])),
        (Some("maps"), Some("find"), 2) => Some(nera_rt_map_find(ctx, arg_terms[0], arg_terms[1])),
        (Some("maps"), Some("put"), 3) => Some(nera_rt_map_put(ctx, arg_terms[2], arg_terms[0], arg_terms[1])),
        (Some("maps"), Some("remove"), 2) => Some(nera_rt_map_remove(ctx, arg_terms[0], arg_terms[1])),
        (Some("maps"), Some("is_key"), 2) | (Some("erlang"), Some("is_map_key"), 2) =>
            Some(nera_rt_map_is_key(ctx, arg_terms[0], arg_terms[1])),
        (Some("maps"), Some("keys"), 1) => Some(nera_rt_map_keys(ctx, arg_terms[0])),
        (Some("maps"), Some("values"), 1) => Some(nera_rt_map_values(ctx, arg_terms[0])),
        (Some("maps"), Some("to_list"), 1) => Some(nera_rt_map_to_list(ctx, arg_terms[0])),
        (Some("maps"), Some("from_list"), 1) => Some(nera_rt_map_from_list(ctx, arg_terms[0])),
        (Some("maps"), Some("from_keys"), 2) => Some(nera_rt_map_from_keys(ctx, arg_terms[0], arg_terms[1])),
        (Some("maps"), Some("size"), 1) => Some(nera_rt_map_size(ctx, arg_terms[0])),
        (Some("maps"), Some("merge"), 2) => Some(nera_rt_map_merge(ctx, arg_terms[0], arg_terms[1])),
        (Some("maps"), Some("update"), 3) => Some(nera_rt_map_update(ctx, arg_terms[2], arg_terms[0], arg_terms[1])),
        (Some("maps"), Some("take"), 2) => Some(nera_rt_map_take(ctx, arg_terms[1], arg_terms[0])),
        (Some("unicode"), Some("characters_to_binary"), 1) => Some(nera_rt_unicode_characters_to_binary(ctx, arg_terms[0])),
        (Some("unicode"), Some("characters_to_binary"), 2) => Some(nera_rt_unicode_characters_to_binary_2(ctx, arg_terms[0], arg_terms[1])),
        (Some("unicode"), Some("characters_to_binary"), 3) => Some(nera_rt_unicode_characters_to_binary_3(ctx, arg_terms[0], arg_terms[1], arg_terms[2])),
        (Some("unicode"), Some("characters_to_list"), 1) => Some(nera_rt_unicode_characters_to_list(ctx, arg_terms[0])),
        (Some("unicode"), Some("characters_to_list"), 2) => Some(nera_rt_unicode_characters_to_list_2(ctx, arg_terms[0], arg_terms[1])),
        (Some("io"), Some("put_chars"), 1) => Some(nera_rt_io_put_chars(ctx, arg_terms[0])),
        (Some("io"), Some("format"), 1) => Some(nera_rt_io_format_1(ctx, arg_terms[0])),
        (Some("io"), Some("format" | "fwrite"), 2) => Some(nera_rt_io_format(ctx, arg_terms[0], arg_terms[1])),
        (Some("io_lib"), Some("format" | "fwrite"), 2) => Some(nera_rt_io_lib_format(ctx, arg_terms[0], arg_terms[1])),
        // error_logger module
        (Some("error_logger"), Some("info_msg"), 2) => Some(nera_rt_error_logger_info_msg(ctx, arg_terms[0], arg_terms[1])),
        (Some("error_logger"), Some("warning_msg"), 2) => Some(nera_rt_error_logger_warning_msg(ctx, arg_terms[0], arg_terms[1])),
        (Some("error_logger"), Some("error_msg"), 2) => Some(nera_rt_error_logger_error_msg(ctx, arg_terms[0], arg_terms[1])),
        (Some("os"), Some("getenv"), 1) => Some(nera_rt_os_getenv(ctx, arg_terms[0])),
        (Some("erlang"), Some("unique_integer"), 0) => Some(nera_rt_unique_integer(ctx)),
        (Some("erlang"), Some("put"), 2) => Some(nera_rt_put(ctx, arg_terms[0], arg_terms[1])),
        (Some("erlang"), Some("get"), 0) => Some(nera_rt_get_all(ctx)),
        (Some("erlang"), Some("get"), 1) => Some(nera_rt_get(ctx, arg_terms[0])),
        (Some("erlang"), Some("erase"), 1) => Some(nera_rt_erase(ctx, arg_terms[0])),
        (Some("erlang"), Some("get_keys"), 0) => Some(nera_rt_get_keys(ctx)),
        (Some("erlang"), Some("get_keys"), 1) => Some(nera_rt_get_keys_1(ctx, arg_terms[0])),
        (Some("erlang"), Some("link"), 1) => Some(nera_rt_link(ctx, arg_terms[0])),
        (Some("erlang"), Some("unlink"), 1) => Some(nera_rt_unlink(ctx, arg_terms[0])),
        (Some("erlang"), Some("monitor"), 2) => Some(nera_rt_monitor(ctx, arg_terms[0], arg_terms[1])),
        (Some("erlang"), Some("demonitor"), 1) => Some(nera_rt_demonitor(ctx, arg_terms[0])),
        (Some("erlang"), Some("demonitor"), 2) => Some(nera_rt_demonitor_2(ctx, arg_terms[0], arg_terms[1])),
        (Some("erlang"), Some("process_flag"), 2) => Some(nera_rt_process_flag(ctx, arg_terms[0], arg_terms[1])),
        (Some("erlang"), Some("hibernate"), 3) => Some(nera_rt_hibernate(ctx, arg_terms[0], arg_terms[1], arg_terms[2])),
        (Some("erlang"), Some("spawn_opt"), 2) => Some(nera_rt_spawn_opt_closure(ctx, arg_terms[0], arg_terms[1])),
        (Some("erlang"), Some("spawn_opt"), 4) => Some(nera_rt_spawn_opt(ctx, arg_terms[0], arg_terms[1], arg_terms[2], arg_terms[3])),
        (Some("erlang"), Some("spawn_link"), 1) => Some(nera_rt_spawn_link_closure(ctx, arg_terms[0])),
        (Some("erlang"), Some("spawn_link"), 3) => Some(nera_rt_spawn_link(ctx, arg_terms[0], arg_terms[1], arg_terms[2])),
        (Some("erlang"), Some("spawn_monitor"), 1) => Some(nera_rt_spawn_monitor_closure(ctx, arg_terms[0])),
        (Some("erlang"), Some("spawn_monitor"), 3) => Some(nera_rt_spawn_monitor(ctx, arg_terms[0], arg_terms[1], arg_terms[2])),
        (Some("erlang"), Some("register"), 2) => Some(nera_rt_register(ctx, arg_terms[0], arg_terms[1])),
        (Some("erlang"), Some("whereis"), 1) => Some(nera_rt_whereis(ctx, arg_terms[0])),
        (Some("erlang"), Some("unregister"), 1) => Some(nera_rt_unregister(ctx, arg_terms[0])),
        (Some("erlang"), Some("registered"), 0) => Some(nera_rt_registered(ctx)),
        // lists module
        (Some("lists"), Some("reverse"), 1) => Some(nera_rt_lists_reverse(ctx, arg_terms[0])),
        (Some("lists"), Some("reverse"), 2) => Some(nera_rt_lists_reverse_2(ctx, arg_terms[0], arg_terms[1])),
        (Some("lists"), Some("member"), 2) => Some(nera_rt_lists_member(ctx, arg_terms[0], arg_terms[1])),
        (Some("lists"), Some("keyfind"), 3) => Some(nera_rt_lists_keyfind(ctx, arg_terms[0], arg_terms[1], arg_terms[2])),
        (Some("lists"), Some("keymember"), 3) => Some(nera_rt_lists_keymember(ctx, arg_terms[0], arg_terms[1], arg_terms[2])),
        (Some("lists"), Some("keysearch"), 3) => Some(nera_rt_lists_keysearch(ctx, arg_terms[0], arg_terms[1], arg_terms[2])),
        (Some("lists"), Some("append"), 1) => Some(nera_rt_lists_append(ctx, arg_terms[0])),
        (Some("lists"), Some("last"), 1) => Some(nera_rt_lists_last(ctx, arg_terms[0])),
        (Some("lists"), Some("nth"), 2) => Some(nera_rt_lists_nth(ctx, arg_terms[0], arg_terms[1])),
        // ets module
        (Some("ets"), Some("new"), 2) => Some(nera_rt_ets_new(ctx, arg_terms[0], arg_terms[1])),
        (Some("ets"), Some("insert"), 2) => Some(nera_rt_ets_insert(ctx, arg_terms[0], arg_terms[1])),
        (Some("ets"), Some("lookup"), 2) => Some(nera_rt_ets_lookup(ctx, arg_terms[0], arg_terms[1])),
        (Some("ets"), Some("delete"), 2) => Some(nera_rt_ets_delete(ctx, arg_terms[0], arg_terms[1])),
        (Some("ets"), Some("delete"), 1) => Some(nera_rt_ets_delete_table(ctx, arg_terms[0])),
        (Some("ets"), Some("delete_all_objects"), 1) => Some(nera_rt_ets_delete_all_objects(ctx, arg_terms[0])),
        (Some("ets"), Some("update_element"), 3) => Some(nera_rt_ets_update_element(ctx, arg_terms[0], arg_terms[1], arg_terms[2])),
        (Some("ets"), Some("update_counter"), 3) => Some(nera_rt_ets_update_counter(ctx, arg_terms[0], arg_terms[1], arg_terms[2])),
        (Some("ets"), Some("select_delete"), 2) => Some(nera_rt_ets_select_delete(ctx, arg_terms[0], arg_terms[1])),
        (Some("ets"), Some("member"), 2) => Some(nera_rt_ets_member(ctx, arg_terms[0], arg_terms[1])),
        (Some("ets"), Some("insert_new"), 2) => Some(nera_rt_ets_insert_new(ctx, arg_terms[0], arg_terms[1])),
        (Some("ets"), Some("tab2list"), 1) => Some(nera_rt_ets_tab2list(ctx, arg_terms[0])),
        (Some("ets"), Some("first"), 1) => Some(nera_rt_ets_first(ctx, arg_terms[0])),
        (Some("ets"), Some("next"), 2) => Some(nera_rt_ets_next(ctx, arg_terms[0], arg_terms[1])),
        (Some("ets"), Some("last"), 1) => Some(nera_rt_ets_last(ctx, arg_terms[0])),
        (Some("ets"), Some("prev"), 2) => Some(nera_rt_ets_prev(ctx, arg_terms[0], arg_terms[1])),
        (Some("ets"), Some("info"), 1) => Some(nera_rt_ets_info_1(ctx, arg_terms[0])),
        (Some("ets"), Some("info"), 2) => Some(nera_rt_ets_info_2(ctx, arg_terms[0], arg_terms[1])),
        (Some("ets"), Some("give_away"), 3) => Some(nera_rt_ets_give_away(ctx, arg_terms[0], arg_terms[1], arg_terms[2])),
        // persistent_term module
        (Some("persistent_term"), Some("put"), 2) => Some(nera_rt_persistent_term_put(ctx, arg_terms[0], arg_terms[1])),
        (Some("persistent_term"), Some("get"), 1) => Some(nera_rt_persistent_term_get(ctx, arg_terms[0])),
        (Some("persistent_term"), Some("get"), 2) => Some(nera_rt_persistent_term_get_2(ctx, arg_terms[0], arg_terms[1])),
        (Some("persistent_term"), Some("erase"), 1) => Some(nera_rt_persistent_term_erase(ctx, arg_terms[0])),
        (Some("persistent_term"), Some("info"), 0) => Some(nera_rt_persistent_term_info(ctx)),
        // counters module
        (Some("counters"), Some("new"), 2) => Some(nera_rt_counters_new(ctx, arg_terms[0], arg_terms[1])),
        (Some("counters"), Some("get"), 2) => Some(nera_rt_counters_get(ctx, arg_terms[0], arg_terms[1])),
        (Some("counters"), Some("put"), 3) => Some(nera_rt_counters_put(ctx, arg_terms[0], arg_terms[1], arg_terms[2])),
        (Some("counters"), Some("add"), 3) => Some(nera_rt_counters_add(ctx, arg_terms[0], arg_terms[1], arg_terms[2])),
        (Some("counters"), Some("sub"), 3) => Some(nera_rt_counters_sub(ctx, arg_terms[0], arg_terms[1], arg_terms[2])),
        (Some("counters"), Some("info"), 1) => Some(nera_rt_counters_info(ctx, arg_terms[0])),
        // zlib module
        (Some("zlib"), Some("open"), 0) => Some(nera_rt_zlib_open(ctx)),
        (Some("zlib"), Some("close"), 1) => Some(nera_rt_zlib_close(ctx, arg_terms[0])),
        (Some("zlib"), Some("deflateInit"), 2) => Some(nera_rt_zlib_deflate_init_2(ctx, arg_terms[0], arg_terms[1])),
        (Some("zlib"), Some("deflateInit"), 6) => Some(nera_rt_zlib_deflate_init(ctx, arg_terms[0], arg_terms[1], arg_terms[2], arg_terms[3], arg_terms[4], arg_terms[5])),
        (Some("zlib"), Some("deflate"), 2) => Some(nera_rt_zlib_deflate_2(ctx, arg_terms[0], arg_terms[1])),
        (Some("zlib"), Some("deflate"), 3) => Some(nera_rt_zlib_deflate(ctx, arg_terms[0], arg_terms[1], arg_terms[2])),
        (Some("zlib"), Some("deflateReset"), 1) => Some(nera_rt_zlib_deflate_reset(ctx, arg_terms[0])),
        (Some("zlib"), Some("deflateEnd"), 1) => Some(nera_rt_zlib_deflate_end(ctx, arg_terms[0])),
        (Some("zlib"), Some("inflateInit"), 1) => Some(nera_rt_zlib_inflate_init_1(ctx, arg_terms[0])),
        (Some("zlib"), Some("inflateInit"), 2) => Some(nera_rt_zlib_inflate_init(ctx, arg_terms[0], arg_terms[1])),
        (Some("zlib"), Some("inflate"), 2) => Some(nera_rt_zlib_inflate(ctx, arg_terms[0], arg_terms[1])),
        (Some("zlib"), Some("safeInflate"), 2) => Some(nera_rt_zlib_safe_inflate(ctx, arg_terms[0], arg_terms[1])),
        (Some("zlib"), Some("inflateReset"), 1) => Some(nera_rt_zlib_inflate_reset(ctx, arg_terms[0])),
        (Some("zlib"), Some("inflateEnd"), 1) => Some(nera_rt_zlib_inflate_end(ctx, arg_terms[0])),
        (Some("zlib"), Some("set_controlling_process"), 2) => Some(nera_rt_zlib_set_controlling_process(ctx, arg_terms[0], arg_terms[1])),
        // atomics module
        (Some("atomics"), Some("new"), 2) => Some(nera_rt_atomics_new(ctx, arg_terms[0], arg_terms[1])),
        (Some("atomics"), Some("get"), 2) => Some(nera_rt_atomics_get(ctx, arg_terms[0], arg_terms[1])),
        (Some("atomics"), Some("put"), 3) => Some(nera_rt_atomics_put(ctx, arg_terms[0], arg_terms[1], arg_terms[2])),
        (Some("atomics"), Some("add"), 3) => Some(nera_rt_atomics_add(ctx, arg_terms[0], arg_terms[1], arg_terms[2])),
        (Some("atomics"), Some("sub"), 3) => Some(nera_rt_atomics_sub(ctx, arg_terms[0], arg_terms[1], arg_terms[2])),
        (Some("atomics"), Some("add_get"), 3) => Some(nera_rt_atomics_add_get(ctx, arg_terms[0], arg_terms[1], arg_terms[2])),
        (Some("atomics"), Some("sub_get"), 3) => Some(nera_rt_atomics_sub_get(ctx, arg_terms[0], arg_terms[1], arg_terms[2])),
        (Some("atomics"), Some("exchange"), 3) => Some(nera_rt_atomics_exchange(ctx, arg_terms[0], arg_terms[1], arg_terms[2])),
        (Some("atomics"), Some("compare_exchange"), 4) => Some(nera_rt_atomics_compare_exchange(ctx, arg_terms[0], arg_terms[1], arg_terms[2], arg_terms[3])),
        (Some("atomics"), Some("info"), 1) => Some(nera_rt_atomics_info(ctx, arg_terms[0])),
        // binary module
        (Some("binary"), Some("split"), 2) => Some(nera_rt_binary_split_2(ctx, arg_terms[0], arg_terms[1])),
        (Some("binary"), Some("split"), 3) => Some(nera_rt_binary_split_3(ctx, arg_terms[0], arg_terms[1], arg_terms[2])),
        (Some("binary"), Some("part"), 3) => Some(nera_rt_binary_part(ctx, arg_terms[0], arg_terms[1], arg_terms[2])),
        (Some("string"), Some("lowercase"), 1) => Some(nera_rt_string_lowercase(ctx, arg_terms[0])),
        (Some("string"), Some("slice"), 3) => Some(nera_rt_string_slice(ctx, arg_terms[0], arg_terms[1], arg_terms[2])),
        (Some("string"), Some("trim"), 3) => Some(nera_rt_string_trim(ctx, arg_terms[0], arg_terms[1], arg_terms[2])),
        (Some("binary"), Some("last"), 1) => Some(nera_rt_binary_last(ctx, arg_terms[0])),
        (Some("binary"), Some("longest_common_prefix"), 1) => Some(nera_rt_binary_longest_common_prefix(ctx, arg_terms[0])),
        (Some("binary"), Some("match"), 3) => Some(nera_rt_binary_match_3(ctx, arg_terms[0], arg_terms[1], arg_terms[2])),
        (Some("binary"), Some("match"), 2) => Some(nera_rt_binary_match(ctx, arg_terms[0], arg_terms[1])),
        (Some("binary"), Some("at"), 2) => Some(nera_rt_binary_at(ctx, arg_terms[0], arg_terms[1])),
        (Some("binary"), Some("referenced_byte_size"), 1) => Some(nera_rt_binary_referenced_byte_size(ctx, arg_terms[0])),
        (Some("binary"), Some("copy"), 1) => Some(nera_rt_binary_copy_1(ctx, arg_terms[0])),
        (Some("binary"), Some("copy"), 2) => Some(nera_rt_binary_copy_2(ctx, arg_terms[0], arg_terms[1])),
        // gen_tcp extensions
        (Some("gen_tcp"), Some("recv"), 3) => Some(nera_rt_gen_tcp_recv_3(ctx, arg_terms[0], arg_terms[1], arg_terms[2])),
        (Some("gen_tcp"), Some("controlling_process"), 2) => Some(nera_rt_gen_tcp_controlling_process(ctx, arg_terms[0], arg_terms[1])),
        // inet module
        (Some("inet"), Some("setopts"), 2) => Some(nera_rt_inet_setopts(ctx, arg_terms[0], arg_terms[1])),
        // nera module — runtime diagnostics
        (Some("nera"), Some("memory_stats"), 0) => Some(nera_rt_memory_stats(ctx)),
        _ => None,
    };

    if let Some(val) = result {
        return val;
    }

    // Function not found — raise error:undef (matching BEAM behaviour)
    let undef_atom = ErlTerm::atom(global_atom_table().intern("undef"));
    capture_exception_trace(ctx);
    if !ctx.is_null() {
        unsafe {
            (*ctx).status = crate::process::STATUS_EXCEPTION;
            (*ctx).result = undef_atom.0;
            (*ctx).exception_class = 1; // error
        }
    }
    ErlTerm::nil().0
}

