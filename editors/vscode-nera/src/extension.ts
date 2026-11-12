import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;

export function activate(context: vscode.ExtensionContext) {
  const config = vscode.workspace.getConfiguration("nera");
  const neraPath = config.get<string>("path", "nera");

  const serverOptions: ServerOptions = {
    command: neraPath,
    args: ["lsp", "--transport", "stdio"],
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      { scheme: "file", language: "erlang" },
    ],
  };

  client = new LanguageClient("nera", "Nera LSP", serverOptions, clientOptions);
  client.start();
}

export function deactivate(): Thenable<void> | undefined {
  return client?.stop();
}
