//! CLI regression for `nera build` with explicit `.erl` files and no `--src`:
//! those files, plus the stdlib and dependencies, are the whole build, whatever
//! folder the command runs from.
//!
//! Bug fixed: the current folder's `tests/` was added to such builds. Building
//! `demo/serverless/handler.erl` from the repository root also compiled every
//! fixture under `./tests` and failed on their unresolved calls, while the same
//! command run from another folder succeeded.

use std::path::Path;
use std::process::Command;

const NERA: &str = env!("CARGO_BIN_EXE_nera");

const GREETING: &str = "\
-module(greeting).
-export([start/0]).

start() ->
    io:format(\"Hello from an explicit build~n\").
";

/// A test suite for a library this build does not include, as in a project's
/// `tests/` folder: it cannot compile on its own.
const COOKIE_TESTS: &str = "\
-module(greeting_cookie_tests).
-export([parse_test/0]).

parse_test() ->
    [{<<\"lang\">>, <<\"fr\">>}] = cow_cookie:parse_cookie(<<\"lang=fr\">>),
    ok.
";

fn write_folder_with_tests(dir: &Path) {
    std::fs::create_dir_all(dir.join("tests")).unwrap();
    std::fs::write(dir.join("tests/greeting_cookie_tests.erl"), COOKIE_TESTS).unwrap();
}

#[test]
fn explicit_files_leave_the_current_folders_tests_out() {
    let tmp = tempfile::tempdir().unwrap();
    // The source sits in a folder with its own tests/, like the repository root.
    let project = tmp.path().join("project");
    write_folder_with_tests(&project);
    std::fs::write(project.join("greeting.erl"), GREETING).unwrap();
    // An unrelated folder that also has a tests/.
    let elsewhere = tmp.path().join("elsewhere");
    write_folder_with_tests(&elsewhere);

    let absolute_source = project.join("greeting.erl");
    let runs: [(&Path, &Path); 2] = [
        (&project, Path::new("greeting.erl")),
        (&elsewhere, &absolute_source),
    ];
    for (cwd, source) in runs {
        // --emit-obj writes one object per compiled module into the current
        // folder and stops before linking.
        let output = Command::new(NERA)
            .current_dir(cwd)
            .arg("build")
            .arg(source)
            .args(["-e", "greeting:start", "--emit-obj"])
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "build of {} from {} failed:\n{stderr}",
            source.display(),
            cwd.display()
        );
        assert!(cwd.join("greeting.o").is_file(), "greeting.o missing in {}", cwd.display());
        assert!(
            !cwd.join("greeting_cookie_tests.o").exists(),
            "tests/ of {} was compiled into the build:\n{stderr}",
            cwd.display()
        );
    }
}
