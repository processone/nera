package net.processone.nera

import com.intellij.codeInspection.InspectionManager
import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.lang.annotation.HighlightSeverity
import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiFile

/**
 * Bridge between the Nera LSP diagnostic cache and the JetBrains
 * inspection framework.
 *
 * The JetBrains platform shows LSP `publishDiagnostics` results in the
 * **File** problems tab (editor annotations), but the **Project Errors**
 * tab is fed exclusively by inspection tools.  This class reads from
 * [NeraLspDiagnosticCache] (populated in [NeraLspServerDescriptor]'s
 * `createAnnotation` override) and surfaces the same diagnostics as
 * [ProblemDescriptor] objects so they appear in **Project Errors**.
 */
class NeraLspInspection : LocalInspectionTool() {

    override fun getDisplayName(): String = "Nera LSP Diagnostics"
    override fun getGroupDisplayName(): String = "Erlang"
    override fun getShortName(): String = "NeraLspDiagnostics"
    override fun isEnabledByDefault(): Boolean = true
    override fun runForWholeFile(): Boolean = true

    override fun checkFile(
        file: PsiFile,
        manager: InspectionManager,
        isOnTheFly: Boolean,
    ): Array<ProblemDescriptor>? {
        val vFile = file.virtualFile ?: return null
        val cache = file.project.getService(NeraLspDiagnosticCache::class.java)
        val entries = cache.get(vFile.url)
        if (entries.isNullOrEmpty()) return null

        val fileLength = file.textLength
        return entries.mapNotNull { entry ->
            // Guard against stale offsets after an edit.
            if (entry.endOffset > fileLength) return@mapNotNull null

            val range = TextRange(entry.startOffset, entry.endOffset)
            val highlightType = when {
                entry.severity >= HighlightSeverity.ERROR   -> ProblemHighlightType.ERROR
                entry.severity >= HighlightSeverity.WARNING -> ProblemHighlightType.WARNING
                else -> ProblemHighlightType.WEAK_WARNING
            }
            manager.createProblemDescriptor(
                file, range, entry.message, highlightType, isOnTheFly,
            )
        }.toTypedArray()
    }
}
