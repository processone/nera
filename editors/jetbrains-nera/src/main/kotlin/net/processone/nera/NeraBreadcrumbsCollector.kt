package net.processone.nera

import com.intellij.codeInsight.breadcrumbs.FileBreadcrumbsCollector
import com.intellij.openapi.Disposable
import com.intellij.openapi.editor.Document
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.editor.event.DocumentEvent
import com.intellij.openapi.editor.event.DocumentListener
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.platform.lsp.api.LspServerManager
import com.intellij.ui.components.breadcrumbs.Crumb
import org.eclipse.lsp4j.DocumentSymbol
import org.eclipse.lsp4j.DocumentSymbolParams
import org.eclipse.lsp4j.Position
import org.eclipse.lsp4j.TextDocumentIdentifier
import java.util.concurrent.ConcurrentHashMap

/**
 * Supplies breadcrumb trails for Erlang files, showing the current scope
 * context: `module > function/arity > clause`. The structure is fetched
 * from the Nera LSP server's `textDocument/documentSymbol` response and
 * cached per document modification stamp.
 */
@Suppress("UnstableApiUsage")
class NeraBreadcrumbsCollector(private val project: Project) : FileBreadcrumbsCollector() {

    private data class CachedSymbols(val stamp: Long, val symbols: List<DocumentSymbol>)

    private val cache = ConcurrentHashMap<VirtualFile, CachedSymbols>()

    override fun handlesFile(virtualFile: VirtualFile): Boolean {
        val ext = virtualFile.extension ?: return false
        return ext == "erl" || ext == "hrl" || ext == "escript"
    }

    override fun watchForChanges(
        file: VirtualFile,
        editor: Editor,
        disposable: Disposable,
        changesHandler: Runnable,
    ) {
        editor.document.addDocumentListener(object : DocumentListener {
            override fun documentChanged(event: DocumentEvent) {
                cache.remove(file)
                changesHandler.run()
            }
        }, disposable)
    }

    override fun computeCrumbs(
        file: VirtualFile,
        document: Document,
        offset: Int,
        forcedShown: Boolean?,
    ): Iterable<Crumb> {
        val symbols = getDocumentSymbols(file, document) ?: return emptyList()

        // Find the deepest path of symbols whose range contains `offset`.
        val crumbs = mutableListOf<Crumb>()

        // First crumb: the module name (= file base name by convention).
        val moduleName = file.nameWithoutExtension
        crumbs.add(Crumb.Impl(null, moduleName, null, null))

        // Walk the symbol tree, matching the symbol whose range contains `offset`.
        var children: List<DocumentSymbol> = symbols
        while (true) {
            val match = children.firstOrNull { containsOffset(it, document, offset) } ?: break
            val label = crumbLabelFor(match)
            crumbs.add(Crumb.Impl(null, label, null, null))
            children = match.children ?: break
        }

        return if (crumbs.size <= 1) emptyList() else crumbs
    }

    private fun crumbLabelFor(symbol: DocumentSymbol): String {
        // The LSP server gives clauses a label like `foo(X, Y)  [1]`. For
        // breadcrumbs we keep it tight: keep the function/arity for the
        // top-level function, and the parameter list for the clause (minus
        // the trailing `[n]` index marker).
        val name = symbol.name
        val idx = name.indexOf("  [")
        return if (idx >= 0) name.substring(0, idx) else name
    }

    private fun containsOffset(symbol: DocumentSymbol, document: Document, offset: Int): Boolean {
        val range = symbol.range ?: return false
        val start = positionToOffset(document, range.start) ?: return false
        val end = positionToOffset(document, range.end) ?: return false
        return offset in start..end
    }

    private fun positionToOffset(document: Document, position: Position): Int? {
        val line = position.line
        if (line < 0 || line >= document.lineCount) return null
        val lineStart = document.getLineStartOffset(line)
        val lineEnd = document.getLineEndOffset(line)
        val col = position.character
        return (lineStart + col).coerceIn(lineStart, lineEnd)
    }

    private fun getDocumentSymbols(file: VirtualFile, document: Document): List<DocumentSymbol>? {
        val stamp = document.modificationStamp
        cache[file]?.let { if (it.stamp == stamp) return it.symbols }

        val lspServer = LspServerManager.getInstance(project)
            .getServersForProvider(NeraLspServerSupportProvider::class.java)
            .firstOrNull() ?: return null

        val uri = lspServer.getDocumentIdentifier(file).uri
        val params = DocumentSymbolParams(TextDocumentIdentifier(uri))

        val result = try {
            lspServer.sendRequestSync { it.textDocumentService.documentSymbol(params) }
        } catch (_: Exception) {
            return null
        } ?: return null

        val symbols = result.filter { it.isRight }.map { it.right }
        cache[file] = CachedSymbols(stamp, symbols)
        return symbols
    }
}
