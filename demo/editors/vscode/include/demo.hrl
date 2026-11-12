-define(APP_NAME, "nera-vscode-demo").

-record(demo_state, {
    editor = "VS Code",
    compiler = nera
}).
