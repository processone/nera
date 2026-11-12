//! I/O formatting: `io:format`, `io_lib:format`, `error_logger`, and term pretty-printing.

#![allow(unused_unsafe)]

use crate::atom_table::global_atom_table;
use crate::bif;
use crate::process::ProcessContext;
use crate::term::*;
use super::helpers::*;


/// Format an Erlang term in a human-friendly way.
///
/// Key differences from `bif::format_term`:
/// - Binaries containing only printable ASCII are shown as `<<"hello">>`
///   instead of `<<104,101,108,108,111>>`
/// - Strings (lists of printable chars) are shown as `"hello"` instead of
///   `[104,101,108,108,111]`
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
unsafe fn format_term_pretty(term: ErlTerm) -> String {
    if term.is_nil() {
        return "[]".to_string();
    }
    if let Some(val) = term.as_small_int() {
        return val.to_string();
    }
    if let Some(id) = term.as_atom_id() {
        let table = global_atom_table();
        let name = table.resolve(id).unwrap_or_else(|| format!("atom#{}", id));
        // Quote atoms that need quoting (contain special chars, start with uppercase, etc.)
        if needs_quoting(&name) {
            return format!("'{}'", name);
        }
        return name;
    }
    if let Some(pid) = term.as_pid_id() {
        return format!("<0.{}.0>", pid);
    }
    if term.is_cons() {
        // Check if it's a printable string (list of chars 32..126)
        if let Some(s) = try_as_printable_string(term) {
            return format!("\"{}\"", s);
        }
        return format_list_pretty(term);
    }
    if term.is_binary() {
        return format_binary_pretty(term);
    }
    if term.is_boxed() {
        return format_boxed_pretty(term);
    }
    format!("#unknown<{:#x}>", term.0)
}

/// Check if an atom name needs single-quote quoting.
fn needs_quoting(name: &str) -> bool {
    if name.is_empty() {
        return true;
    }
    let first = name.chars().next().unwrap();
    // Atoms starting with uppercase or containing special chars need quoting
    if first.is_uppercase() || first == '_' {
        return true;
    }
    // Check for special characters
    name.chars().any(|c| !c.is_alphanumeric() && c != '_' && c != '@')
}

/// Try to interpret a list as a printable ASCII string.
/// # Safety
/// The complete input term graph must be initialized, live and unmoved by GC
/// until this call returns. Binary backing bytes must not be concurrently mutated.
unsafe fn try_as_printable_string(term: ErlTerm) -> Option<String> {
    let mut chars = Vec::new();
    let mut cursor = term;
    while cursor.is_cons() {
        {
            let ptr = cursor.as_cons_ptr()?;
            unsafe {
                let head = (*ptr).head;
                {
                    let val = head.as_small_int()?;
                    if (32..=126).contains(&val) {
                        chars.push(val as u8 as char);
                    } else {
                        return None;
                    }
                }
                cursor = (*ptr).tail;
            }
        }
    }
    if !cursor.is_nil() || chars.is_empty() {
        return None;
    }
    Some(chars.into_iter().collect())
}

/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
unsafe fn format_list_pretty(term: ErlTerm) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut cursor = term;
    while cursor.is_cons() {
        unsafe {
            let cell = &*cursor.as_cons_ptr().unwrap();
            parts.push(format_term_pretty(cell.head));
            cursor = cell.tail;
        }
    }
    if cursor.is_nil() {
        format!("[{}]", parts.join(", "))
    } else {
        format!("[{} | {}]", parts.join(", "), format_term_pretty(cursor))
    }
}

/// Format a binary in human-friendly way:
/// - Printable ASCII: `<<"hello">>`
/// - Otherwise: `<<104, 101, 108>>`
/// # Safety
/// The complete input term graph must be initialized, live and unmoved by GC
/// until this call returns. Binary backing bytes must not be concurrently mutated.
unsafe fn format_binary_pretty(term: ErlTerm) -> String {
    let (ptr, len) = match unsafe { binary_data(term) } {
        Some(data) => data,
        None => return "<<>>".to_string(),
    };
    if len == 0 {
        return "<<>>".to_string();
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };

    // Check if all bytes are printable ASCII
    let all_printable = bytes.iter().all(|&b| (32..=126).contains(&b) || b == b'\n' || b == b'\r' || b == b'\t');

    if all_printable && len > 0 {
        let s = String::from_utf8_lossy(bytes);
        format!("<<\"{}\">>", s)
    } else if len <= 32 {
        let parts: Vec<String> = bytes.iter().map(|b| format!("{}", b)).collect();
        format!("<<{}>>", parts.join(", "))
    } else {
        // Large non-printable binary: show first 16 bytes + ellipsis
        let parts: Vec<String> = bytes[..16].iter().map(|b| format!("{}", b)).collect();
        format!("<<{}, ...>> ({} bytes)", parts.join(", "), len)
    }
}

/// # Safety
/// The complete input term graph must be initialized, live and unmoved by GC
/// until this call returns. Binary backing bytes must not be concurrently mutated.
unsafe fn format_boxed_pretty(term: ErlTerm) -> String {
    unsafe {
        let ptr = match term.as_boxed_ptr() {
            Some(p) => p,
            None => return "#unknown".to_string(),
        };
        let header = crate::term::BoxedHeader(*ptr);
        let arity = header.arity() as usize;

        match header.subtype() {
            crate::term::BOXED_TUPLE => {
                let mut elems: Vec<String> = Vec::with_capacity(arity);
                for i in 0..arity {
                    elems.push(format_term_pretty(ErlTerm(*ptr.add(1 + i))));
                }
                format!("{{{}}}", elems.join(", "))
            }
            crate::term::BOXED_FLOAT => {
                let bits = *ptr.add(1);
                let val = f64::from_bits(bits);
                if val.fract() == 0.0 && val.is_finite() {
                    format!("{:.1}", val)
                } else {
                    format!("{}", val)
                }
            }
            crate::term::BOXED_MAP => {
                let npairs = arity;
                let mut parts: Vec<String> = Vec::with_capacity(npairs);
                for i in 0..npairs {
                    let key = ErlTerm(*ptr.add(1 + i * 2));
                    let val = ErlTerm(*ptr.add(1 + i * 2 + 1));
                    parts.push(format!("{} => {}", format_term_pretty(key), format_term_pretty(val)));
                }
                format!("#{{{}}}", parts.join(", "))
            }
            crate::term::BOXED_CLOSURE => {
                let fun_arity = *ptr.add(2);
                format!("#Fun</{}>", fun_arity)
            }
            crate::term::BOXED_BIGINT => {
                match crate::bigint::term_to_bigint(term) {
                    Some(val) => val.to_string(),
                    None => format!("#bigint<limbs={}>", arity),
                }
            }
            crate::term::BOXED_REFERENCE => {
                let id = *ptr.add(1);
                format!("#Ref<0.0.0.{}>", id)
            }
            _ => format!("#boxed<type={}, arity={}>", header.subtype(), arity),
        }
    }
}

/// Format an Erlang format string + args to a Rust String using `~p`, `~s`, `~w`, `~n`.
/// # Safety
/// All input terms and reachable objects must remain live and unmoved by GC
/// for this call. Binary backing bytes must not be concurrently mutated.
unsafe fn format_erlang_string(fmt: &str, args: &[ErlTerm]) -> String {
    let mut output = String::new();
    let mut arg_idx = 0;
    let mut chars = fmt.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '~' {
            match chars.peek() {
                Some(&'p') | Some(&'w') => {
                    chars.next();
                    if arg_idx < args.len() {
                        output.push_str(&format_term_pretty(args[arg_idx]));
                        arg_idx += 1;
                    }
                }
                Some(&'s') => {
                    chars.next();
                    if arg_idx < args.len() {
                        let term = args[arg_idx];
                        if term.is_binary() {
                            if let Some((ptr, len)) = unsafe { binary_data(term) } {
                                let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
                                output.push_str(&String::from_utf8_lossy(bytes));
                            }
                        } else if let Some(s) = try_as_printable_string(term) {
                            output.push_str(&s);
                        } else {
                            output.push_str(&format_term_pretty(term));
                        }
                        arg_idx += 1;
                    }
                }
                Some(&'d') | Some(&'B') | Some(&'b') => {
                    chars.next();
                    if arg_idx < args.len() {
                        output.push_str(&format_term_pretty(args[arg_idx]));
                        arg_idx += 1;
                    }
                }
                Some(&'n') => {
                    chars.next();
                    output.push('\n');
                }
                Some(&'~') => {
                    chars.next();
                    output.push('~');
                }
                Some(&c) if c.is_ascii_digit() || c == '-' || c == '.' || c == '*' => {
                    // Width/precision specifier — skip digits then handle format char
                    chars.next();
                    let mut _spec = String::new();
                    _spec.push(c);
                    while let Some(&next) = chars.peek() {
                        if next.is_ascii_digit() || next == '.' || next == '-' || next == '*' {
                            _spec.push(chars.next().unwrap());
                        } else {
                            break;
                        }
                    }
                    if let Some(fmt_char) = chars.next() {
                        match fmt_char {
                            'p' | 'w' | 'd' | 'B' | 'b' | 'f' => {
                                if arg_idx < args.len() {
                                    output.push_str(&format_term_pretty(args[arg_idx]));
                                    arg_idx += 1;
                                }
                            }
                            's' => {
                                if arg_idx < args.len() {
                                    if let Some(s) = try_as_printable_string(args[arg_idx]) {
                                        output.push_str(&s);
                                    } else {
                                        output.push_str(&format_term_pretty(args[arg_idx]));
                                    }
                                    arg_idx += 1;
                                }
                            }
                            'n' => output.push('\n'),
                            _ => {
                                output.push('~');
                                output.push_str(&_spec);
                                output.push(fmt_char);
                            }
                        }
                    }
                }
                Some(&other) => {
                    chars.next();
                    output.push('~');
                    output.push(other);
                }
                None => {
                    output.push('~');
                }
            }
        } else {
            output.push(ch);
        }
    }
    output
}

/// Get a formatted wall-clock timestamp: "HH:MM:SS".
fn format_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Convert to HH:MM:SS (UTC)
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;
    format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
}

/// Check if stderr is a TTY (for color support).
fn is_tty() -> bool {
    use std::io::IsTerminal;
    std::io::stderr().is_terminal()
}

/// io:format/2 - formatted output.
/// fmt_term is an ErlTerm list (string), args_term is an ErlTerm list.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_io_format(ctx: *mut ProcessContext, fmt_term: u64, args_term: u64) -> u64 {
    let result = with_ctx_heap(ctx, |heap| {
        bif::bif_io_format_2(heap, &[ErlTerm(fmt_term), ErlTerm(args_term)])
    });
    match result {
        Ok(val) => val.0,
        Err(_) => raise_badarg(ctx),
    }
}

/// io_lib:format/2 — format a string and return it as an iolist (binary).
/// Unlike io:format/2 which prints to stdout, this returns the formatted string.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_io_lib_format(ctx: *mut ProcessContext, fmt_term: u64, args_term: u64) -> u64 {
    let output = match crate::io_format::format_output(ErlTerm(fmt_term), ErlTerm(args_term)) {
        Ok(output) => output,
        Err(()) => return raise_badarg(ctx),
    };

    // Return as a list of character codes (standard iolist representation)
    with_ctx_heap(ctx, |heap| {
        let codepoints: Vec<ErlTerm> = output.chars().map(|c| ErlTerm::small_int(c as i64)).collect();
        heap.alloc_list_from_slice(&codepoints).0
    })
}

/// io:format/1 - simple output.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_io_format_1(ctx: *mut ProcessContext, fmt_term: u64) -> u64 {
    let result = with_ctx_heap(ctx, |heap| {
        bif::bif_io_format_1(heap, &[ErlTerm(fmt_term)])
    });
    match result {
        Ok(val) => val.0,
        Err(_) => raise_badarg(ctx),
    }
}

// ANSI color escape sequences for log output.
const ANSI_RESET: &str  = "\x1b[0m";
const ANSI_BOLD: &str   = "\x1b[1m";
const ANSI_DIM: &str    = "\x1b[2m";
const ANSI_RED: &str    = "\x1b[31m";
const ANSI_YELLOW: &str = "\x1b[33m";
const ANSI_CYAN: &str   = "\x1b[36m";

/// Core error_logger output function.
///
/// Prints a formatted log line to stderr with optional ANSI colors:
///
///   12:34:56 [info]  <0.42.0> Connection accepted on port 11211
///   12:34:56 [error] <0.42.0> TCP error on #Port<5>: econnreset
///
/// Level: 0=info, 1=warning, 2=error
#[no_mangle]
pub unsafe extern "C" fn nera_rt_error_logger_msg(
    ctx: *mut ProcessContext,
    level_term: u64,
    fmt_term: u64,
    args_term: u64,
) -> u64 {
    let level = ErlTerm(level_term).as_small_int().unwrap_or(0);

    // Parse the format string — could be a list (string) or binary
    let fmt_str = {
        let fmt = ErlTerm(fmt_term);
        if fmt.is_binary() {
            match unsafe { binary_data(fmt) } {
                Some((ptr, len)) => {
                    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
                    String::from_utf8_lossy(bytes).into_owned()
                }
                None => String::new(),
            }
        } else {
            bif::term_to_string(fmt).unwrap_or_default()
        }
    };

    // Collect args
    let mut args: Vec<ErlTerm> = Vec::new();
    let mut cursor = ErlTerm(args_term);
    while cursor.is_cons() {
        if let Some(ptr) = cursor.as_cons_ptr() {
            unsafe {
                args.push((*ptr).head);
                cursor = (*ptr).tail;
            }
        } else {
            break;
        }
    }

    // Format the message body
    let body = format_erlang_string(&fmt_str, &args);

    // Get metadata
    let timestamp = format_timestamp();
    let pid = if !ctx.is_null() {
        unsafe { (*ctx).pid }
    } else {
        crate::process::ProcessId::new(0)
    };

    let use_color = is_tty();

    // Build the log line
    let (level_tag, level_color) = match level {
        0 => ("info ", ANSI_CYAN),
        1 => ("warn ", ANSI_YELLOW),
        _ => ("error", ANSI_RED),
    };

    // Trim trailing newlines from body (we add our own)
    let body = body.trim_end_matches('\n');

    if use_color {
        eprintln!(
            "{dim}{ts}{reset} {color}{bold}[{tag}]{reset} {dim}<0.{pid}.0>{reset} {body}",
            dim = ANSI_DIM,
            ts = timestamp,
            reset = ANSI_RESET,
            color = level_color,
            bold = ANSI_BOLD,
            tag = level_tag,
            pid = pid,
            body = body,
        );
    } else {
        eprintln!(
            "{ts} [{tag}] <0.{pid}.0> {body}",
            ts = timestamp,
            tag = level_tag,
            pid = pid,
            body = body,
        );
    }

    ErlTerm::atom(crate::atom_table::ATOM_OK).0
}

/// error_logger:info_msg/2
#[no_mangle]
pub unsafe extern "C" fn nera_rt_error_logger_info_msg(ctx: *mut ProcessContext, fmt: u64, args: u64) -> u64 {
    nera_rt_error_logger_msg(ctx, ErlTerm::small_int(0).0, fmt, args)
}

/// error_logger:warning_msg/2
#[no_mangle]
pub unsafe extern "C" fn nera_rt_error_logger_warning_msg(ctx: *mut ProcessContext, fmt: u64, args: u64) -> u64 {
    nera_rt_error_logger_msg(ctx, ErlTerm::small_int(1).0, fmt, args)
}

/// error_logger:error_msg/2
#[no_mangle]
pub unsafe extern "C" fn nera_rt_error_logger_error_msg(ctx: *mut ProcessContext, fmt: u64, args: u64) -> u64 {
    nera_rt_error_logger_msg(ctx, ErlTerm::small_int(2).0, fmt, args)
}

/// Write Unicode chardata to the default standard output.
#[no_mangle]
pub unsafe extern "C" fn nera_rt_io_put_chars(ctx: *mut ProcessContext, chars: u64) -> u64 {
    let converted = super::unicode::nera_rt_unicode_characters_to_binary(ctx, chars);
    let Some((ptr, len)) = binary_data(ErlTerm(converted)) else { return raise_badarg(ctx); };
    let bytes = std::slice::from_raw_parts(ptr, len);
    use std::io::Write;
    let mut output = std::io::stdout().lock();
    if output.write_all(bytes).and_then(|_| output.flush()).is_err() { return raise_badarg(ctx); }
    ErlTerm::atom(crate::atom_table::ATOM_OK).0
}

#[cfg(test)]
mod integer_format_tests {
    use super::*;
    use crate::heap::ProcessHeap;

    fn format(fmt: &str, values: &[i64]) -> String {
        let mut heap = ProcessHeap::new();
        let mut ctx = ProcessContext::new(crate::process::ProcessId::new(1));
        ctx.heap_ptr = &mut heap;
        let chars: Vec<_> = fmt.chars().map(|c| ErlTerm::small_int(c as i64)).collect();
        let fmt = heap.alloc_list_from_slice(&chars);
        let values: Vec<_> = values.iter().copied().map(ErlTerm::small_int).collect();
        let args = heap.alloc_list_from_slice(&values);
        // All terms and the context remain live on this exclusively owned heap.
        let result = unsafe { nera_rt_io_lib_format(&mut ctx, fmt.0, args.0) };
        unsafe { bif::term_to_string(ErlTerm(result)) }.unwrap()
    }

    #[test]
    fn integer_width_radix_and_padding_match_beam() {
        for (fmt, value, expected) in [
            ("~4..0b", 0, "0000"),
            ("~0b", 7, ""),
            ("~2..0b", 99, "99"),
            ("~2..0b", 123, "**"),
            ("~4..0b", -7, "00-7"),
            ("~-4..0B", -7, "-700"),
            ("~4..xb", 7, "xxx7"),
            ("~4.16.0B", 255, "00FF"),
            ("~4.16.0b", 255, "00ff"),
            ("~.2b", 10, "1010"),
            ("~4b", 7, "   7"),
            ("~4..λb", 7, "λλλ7"),
            ("~4...b", 7, "...7"),
        ] {
            assert_eq!(format(fmt, &[value]), expected, "{fmt}");
        }
    }

    #[test]
    fn dynamic_integer_fields_consume_arguments_in_order() {
        assert_eq!(format("~*.*.*B|~2..0b", &[4, 16, 48, 255, 7]), "00FF|07");
    }

    #[test]
    fn date_fields_are_zero_padded() {
        assert_eq!(
            format(
                "~4..0b-~2..0b-~2..0bT~2..0b:~2..0b:~2..0b.~6..0bZ",
                &[2023, 8, 7, 19, 2, 24, 65432]
            ),
            "2023-08-07T19:02:24.065432Z"
        );
    }
}
