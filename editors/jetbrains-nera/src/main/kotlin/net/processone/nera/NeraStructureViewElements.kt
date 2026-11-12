package net.processone.nera

import com.intellij.ide.structureView.StructureViewTreeElement
import com.intellij.ide.util.treeView.smartTree.SortableTreeElement
import com.intellij.navigation.ItemPresentation
import com.intellij.openapi.fileEditor.OpenFileDescriptor
import com.intellij.platform.lsp.api.LspServerManager
import com.intellij.psi.PsiFile
import org.eclipse.lsp4j.DocumentSymbol
import org.eclipse.lsp4j.DocumentSymbolParams
import org.eclipse.lsp4j.SymbolKind
import org.eclipse.lsp4j.TextDocumentIdentifier
import javax.swing.Icon

/**
 * Root tree element for the structure view.  Fetches document symbols
 * from the Nera LSP server.
 */
class NeraFileTreeElement(private val psiFile: PsiFile) : StructureViewTreeElement {

    override fun getValue(): Any = psiFile

    override fun getPresentation(): ItemPresentation = object : ItemPresentation {
        override fun getPresentableText(): String = psiFile.name
        override fun getLocationString(): String? = null
        override fun getIcon(unused: Boolean): Icon = NeraIcons.ERLANG_MODULE
    }

    override fun getChildren(): Array<StructureViewTreeElement> {
        val project = psiFile.project
        val vFile = psiFile.virtualFile ?: return emptyArray()

        @Suppress("UnstableApiUsage")
        val lspServer = LspServerManager.getInstance(project)
            .getServersForProvider(NeraLspServerSupportProvider::class.java)
            .firstOrNull() ?: return emptyArray()

        val uri = lspServer.getDocumentIdentifier(vFile).uri
        val params = DocumentSymbolParams(TextDocumentIdentifier(uri))

        val result = try {
            lspServer.sendRequestSync { it.textDocumentService.documentSymbol(params) }
        } catch (_: Exception) {
            return emptyArray()
        } ?: return emptyArray()

        return result
            .filter { it.isRight }
            .map { NeraSymbolTreeElement(it.right, psiFile, false) }
            .toTypedArray()
    }

    override fun navigate(requestFocus: Boolean) {}
    override fun canNavigate(): Boolean = false
    override fun canNavigateToSource(): Boolean = false
}

/**
 * Tree element for an individual document symbol (function, record,
 * clause, or field).
 */
class NeraSymbolTreeElement(
    private val symbol: DocumentSymbol,
    private val psiFile: PsiFile,
    private val isClause: Boolean,
) : StructureViewTreeElement, SortableTreeElement {

    override fun getValue(): Any = symbol

    override fun getAlphaSortKey(): String = symbol.name

    override fun getPresentation(): ItemPresentation = object : ItemPresentation {
        override fun getPresentableText(): String = symbol.name
        override fun getLocationString(): String? = symbol.detail
        override fun getIcon(unused: Boolean): Icon = NeraSymbolIcons.forSymbol(symbol, isClause)
    }

    override fun getChildren(): Array<StructureViewTreeElement> {
        val children = symbol.children ?: return emptyArray()
        // Children of functions are clauses; children of records are fields
        val childIsClause = symbol.kind == SymbolKind.Function && !isClause
        return children.map { NeraSymbolTreeElement(it, psiFile, childIsClause) }.toTypedArray()
    }

    override fun navigate(requestFocus: Boolean) {
        val project = psiFile.project
        val vFile = psiFile.virtualFile ?: return
        val range = symbol.selectionRange ?: symbol.range ?: return
        val document = com.intellij.openapi.fileEditor.FileDocumentManager
            .getInstance().getDocument(vFile) ?: return
        val offset = document.getLineStartOffset(range.start.line) + range.start.character
        OpenFileDescriptor(project, vFile, offset).navigate(requestFocus)
    }

    override fun canNavigate(): Boolean = true
    override fun canNavigateToSource(): Boolean = true
}
