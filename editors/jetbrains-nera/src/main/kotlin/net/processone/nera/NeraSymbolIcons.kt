package net.processone.nera

import com.intellij.icons.AllIcons
import com.intellij.ui.RowIcon
import org.eclipse.lsp4j.DocumentSymbol
import org.eclipse.lsp4j.SymbolKind
import javax.swing.Icon

/**
 * Produces compound icons for structure view symbols with visibility
 * and recursion overlays.
 */
object NeraSymbolIcons {

    fun forSymbol(symbol: DocumentSymbol, isClause: Boolean = false): Icon {
        val detail = symbol.detail ?: ""

        // Base icon
        val base: Icon = when {
            isClause -> AllIcons.Nodes.Lambda
            symbol.kind == SymbolKind.Function -> AllIcons.Nodes.Function
            symbol.kind == SymbolKind.Struct -> AllIcons.Nodes.Class
            symbol.kind == SymbolKind.Field -> AllIcons.Nodes.Field
            else -> AllIcons.Nodes.Function
        }

        // Determine overlays
        val isExported = detail.contains("exported")
        val isRecursive = detail.contains("recursive")

        // Count how many icon slots we need
        val slots = 1 + (if (isExported) 1 else 0) + (if (isRecursive) 1 else 0)

        if (slots == 1) return base

        val row = RowIcon(slots)
        var idx = 0
        row.setIcon(base, idx++)
        if (isExported) {
            row.setIcon(AllIcons.Nodes.C_public, idx++)
        }
        if (isRecursive) {
            row.setIcon(AllIcons.Gutter.RecursiveMethod, idx++)
        }
        return row
    }
}
