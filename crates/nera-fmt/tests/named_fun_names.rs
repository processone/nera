use nera_fmt::{config::FormatConfig, format_source};

fn format(source: &str) -> String {
    format_source(source, &FormatConfig::default()).unwrap()
}

// erlc accepts only a variable as a named fun's name, so quoting one makes the
// formatted file stop compiling.
#[test]
fn variable_names_stay_unquoted_in_every_clause() {
    let source = "count(N) ->
    Loop = fun Loop(0) -> done;
        Loop(M) when M > 0 -> Loop(M - 1)
        end,
    Loop(N).
";
    assert_eq!(format(source), source);
}

#[test]
fn underscore_names_stay_unquoted() {
    let source = "add() -> fun _(N) -> N + 1
    end.
unused() -> fun _BlackAdder(N) -> N + 42
    end.
stack() -> fun _stack@2(N) -> N
    end.
";
    assert_eq!(format(source), source);
}

// Nera also reads an atom as the name. Only a name that is not a valid
// variable still needs its quotes.
#[test]
fn atom_names_keep_quotes_only_when_they_need_them() {
    let formats_to = |name: &str, expected: &str| {
        let source = format!("a() -> fun {name}(N) -> N\n    end.\n");
        let wanted = format!("a() -> fun {expected}(N) -> N\n    end.\n");
        assert_eq!(format(&source), wanted);
    };
    formats_to("'two words'", "'two words'");
    formats_to("plain", "plain");
    formats_to("'Loop'", "Loop");
}
