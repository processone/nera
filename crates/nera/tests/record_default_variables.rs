//! Record default variables are semantic errors in both CLI entry points,
//! even for an unused record and when warnings are suppressed.

use std::process::Command;

const NERA: &str = env!("CARGO_BIN_EXE_nera");

#[test]
fn check_and_build_reject_record_default_variables() {
    let tmp = tempfile::tempdir().unwrap();
    for default in ["begin X = 1, X end", "X"] {
        let source = format!(
            "-module(record_default).\n-export([start/0]).\n\
             -record(r, {{a = {default}}}).\nstart() -> ok.\n"
        );
        std::fs::write(tmp.path().join("record_default.erl"), source).unwrap();
        for command in ["check", "build"] {
            for suppress_warnings in [false, true] {
                let mut cmd = Command::new(NERA);
                cmd.current_dir(tmp.path())
                    .args([command, "record_default.erl", "--no-stdlib"]);
                if command == "build" {
                    cmd.args(["-e", "record_default:start", "--emit-obj"]);
                }
                if suppress_warnings {
                    cmd.arg("--no-warnings");
                }
                let output = cmd.output().unwrap();
                let stderr = String::from_utf8_lossy(&output.stderr);
                assert!(
                    !output.status.success()
                        && stderr.contains("error: variable 'X' in record definition"),
                    "{command}, default {default}, suppressed {suppress_warnings}:\n{stderr}"
                );
                assert!(
                    !tmp.path().join("record_default.o").exists(),
                    "invalid record default reached object emission"
                );
            }
        }
    }
}
