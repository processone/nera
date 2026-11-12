package net.processone.nera

import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.actionSystem.DataContext
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.openapi.ui.Messages
import com.intellij.platform.lsp.api.LspServerManager
import com.intellij.psi.PsiFile
import com.intellij.refactoring.rename.RenameHandler
import org.eclipse.lsp4j.*

/**
 * Custom rename handler that bridges Shift+F6 to the Nera LSP server's
 * textDocument/rename support for Erlang variables.
 *
 * The JetBrains built-in LSP API does not yet support textDocument/rename,
 * so we manually send prepareRename and rename requests via the raw
 * LspServer.sendRequestSync API.
 */
class NeraRenameHandler : RenameHandler {

    override fun isAvailableOnDataContext(dataContext: DataContext): Boolean {
        val editor = CommonDataKeys.EDITOR.getData(dataContext) ?: return false
        val file = CommonDataKeys.PSI_FILE.getData(dataContext) ?: return false
        val vFile = file.virtualFile ?: return false

        // Only handle Erlang files
        if (vFile.extension !in setOf("erl", "hrl", "xrl", "yrl", "escript")) return false

        // Quick check: is the token under the cursor a VARIABLE?
        val offset = editor.caretModel.offset
        val highlighter = editor.highlighter
        var iter = highlighter.createIterator(offset)
        if (iter.atEnd()) return false
        // The iterator may land on the token that starts at or before offset
        return iter.tokenType == ErlangTokenTypes.VARIABLE
    }

    override fun isRenaming(dataContext: DataContext): Boolean = isAvailableOnDataContext(dataContext)

    override fun invoke(project: Project, editor: Editor, file: PsiFile, dataContext: DataContext) {
        val vFile = file.virtualFile ?: return
        val document = editor.document

        val lspServer = LspServerManager.getInstance(project)
            .getServersForProvider(NeraLspServerSupportProvider::class.java)
            .firstOrNull() ?: return

        val offset = editor.caretModel.offset
        val line = document.getLineNumber(offset)
        val character = offset - document.getLineStartOffset(line)
        val uri = lspServer.getDocumentIdentifier(vFile).uri

        // Step 1: prepareRename — check if position is renameable and get current name
        val prepareParams = PrepareRenameParams(TextDocumentIdentifier(uri), Position(line, character))
        val prepareResult = try {
            lspServer.sendRequestSync { it.textDocumentService.prepareRename(prepareParams) }
        } catch (_: Exception) {
            null
        }

        if (prepareResult == null) {
            Messages.showInfoMessage(project, "Cannot rename element at this position.", "Rename")
            return
        }

        // Extract placeholder name from the Either3<Range, PrepareRenameResult, PrepareRenameDefaultBehavior>
        val placeholder = when {
            prepareResult.isSecond -> prepareResult.getSecond().placeholder
            prepareResult.isFirst -> {
                val range = prepareResult.getFirst()
                val startOff = document.getLineStartOffset(range.start.line) + range.start.character
                val endOff = document.getLineStartOffset(range.end.line) + range.end.character
                document.getText(com.intellij.openapi.util.TextRange(startOff, endOff))
            }
            else -> return
        }

        // Step 2: Ask for new name
        val newName = Messages.showInputDialog(
            project,
            "Enter new name:",
            "Rename Variable",
            null,
            placeholder,
            null,
        )
        if (newName.isNullOrBlank() || newName == placeholder) return

        // Step 3: Send rename request
        val renameParams = RenameParams(TextDocumentIdentifier(uri), Position(line, character), newName)
        val workspaceEdit = try {
            lspServer.sendRequestSync { it.textDocumentService.rename(renameParams) }
        } catch (e: Exception) {
            Messages.showErrorDialog(project, e.message ?: "Rename failed", "Rename Error")
            return
        }

        if (workspaceEdit == null) return

        // Step 4: Apply workspace edit
        applyWorkspaceEdit(project, workspaceEdit, document)
    }

    override fun invoke(project: Project, elements: Array<out com.intellij.psi.PsiElement>, dataContext: DataContext) {
        // Delegate to the editor-based invoke; this path is rarely used for LSP
        val editor = CommonDataKeys.EDITOR.getData(dataContext) ?: return
        val file = CommonDataKeys.PSI_FILE.getData(dataContext) ?: return
        invoke(project, editor, file, dataContext)
    }

    private fun applyWorkspaceEdit(
        project: Project,
        edit: WorkspaceEdit,
        currentDocument: com.intellij.openapi.editor.Document,
    ) {
        WriteCommandAction.runWriteCommandAction(project, "Rename Variable", null, {
            // Handle documentChanges (our server uses this format)
            val docChanges = edit.documentChanges
            if (docChanges != null) {
                for (change in docChanges) {
                    if (!change.isLeft) continue
                    val textDocEdit = change.getLeft()
                    val sorted = textDocEdit.edits.sortedByDescending {
                        it.range.start.line * 100000 + it.range.start.character
                    }
                    for (textEdit in sorted) {
                        val startOffset = currentDocument.getLineStartOffset(textEdit.range.start.line) + textEdit.range.start.character
                        val endOffset = currentDocument.getLineStartOffset(textEdit.range.end.line) + textEdit.range.end.character
                        currentDocument.replaceString(startOffset, endOffset, textEdit.newText)
                    }
                }
                return@runWriteCommandAction
            }

            // Fallback: handle changes map
            val changes = edit.changes
            if (changes != null) {
                for ((_, edits) in changes) {
                    val sorted = edits.sortedByDescending {
                        it.range.start.line * 100000 + it.range.start.character
                    }
                    for (textEdit in sorted) {
                        val startOffset = currentDocument.getLineStartOffset(textEdit.range.start.line) + textEdit.range.start.character
                        val endOffset = currentDocument.getLineStartOffset(textEdit.range.end.line) + textEdit.range.end.character
                        currentDocument.replaceString(startOffset, endOffset, textEdit.newText)
                    }
                }
            }
        })
    }
}
