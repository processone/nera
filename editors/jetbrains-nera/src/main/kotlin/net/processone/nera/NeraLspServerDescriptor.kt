package net.processone.nera

import com.intellij.codeInsight.daemon.DaemonCodeAnalyzer
import com.intellij.codeInsight.intention.IntentionAction
import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.lang.annotation.AnnotationHolder
import com.intellij.lang.annotation.HighlightSeverity
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.editor.DefaultLanguageHighlighterColors
import com.intellij.openapi.editor.colors.TextAttributesKey
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.TextRange
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.platform.lsp.api.ProjectWideLspServerDescriptor
import com.intellij.platform.lsp.api.customization.LspCompletionCustomizer
import com.intellij.platform.lsp.api.customization.LspCompletionSupport
import com.intellij.platform.lsp.api.customization.LspCustomization
import com.intellij.platform.lsp.api.customization.LspDiagnosticsCustomizer
import com.intellij.platform.lsp.api.customization.LspDiagnosticsSupport
import com.intellij.platform.lsp.api.customization.LspFormattingCustomizer
import com.intellij.platform.lsp.api.customization.LspFormattingSupport
import com.intellij.platform.lsp.api.customization.LspSemanticTokensCustomizer
import com.intellij.platform.lsp.api.customization.LspSemanticTokensSupport
import com.intellij.problems.WolfTheProblemSolver
import com.intellij.psi.PsiManager
import org.eclipse.lsp4j.Diagnostic
import org.eclipse.lsp4j.DiagnosticSeverity
import java.lang.ref.WeakReference
import java.util.concurrent.ConcurrentHashMap

class NeraLspServerDescriptor(project: Project) : ProjectWideLspServerDescriptor(project, "Nera") {

    override val lspCustomization: LspCustomization = object : LspCustomization() {
        override val formattingCustomizer: LspFormattingCustomizer = object : LspFormattingSupport() {
            override fun shouldFormatThisFileExclusivelyByServer(
                file: VirtualFile,
                ideCanFormatThisFileItself: Boolean,
                serverExplicitlyWantsToFormatThisFile: Boolean
            ): Boolean = true
        }

        override val completionCustomizer: LspCompletionCustomizer = object : LspCompletionSupport() {
            override fun isTriggerCharacterRespected(c: Char): Boolean {
                // In Erlang, "." is a statement terminator, not a member access operator
                if (c == '.') return false
                return super.isTriggerCharacterRespected(c)
            }
        }

        override val diagnosticsCustomizer: LspDiagnosticsCustomizer = object : LspDiagnosticsSupport() {
            /**
             * Track the current [AnnotationSession] so we can detect when
             * the platform starts processing a new file and clear the
             * stale cache entries for it.
             */
            private var lastSession: WeakReference<Any>? = null

            /** Per-file content hash for detecting actual diagnostic changes. */
            private val lastKnownHash = ConcurrentHashMap<String, Int>()

            /** Files with a pending invokeLater restart check (deduplication). */
            private val pendingFiles: MutableSet<String> = ConcurrentHashMap.newKeySet()

            override fun getHighlightSeverity(diagnostic: Diagnostic): HighlightSeverity {
                val code = diagnostic.code?.get()?.toString() ?: ""
                return when (code) {
                    "unused_variable", "unused_function", "shadowed_variable" ->
                        HighlightSeverity.WEAK_WARNING
                    else -> when (diagnostic.severity) {
                        DiagnosticSeverity.Error -> HighlightSeverity.ERROR
                        DiagnosticSeverity.Warning -> HighlightSeverity.WARNING
                        DiagnosticSeverity.Information -> HighlightSeverity.INFORMATION
                        DiagnosticSeverity.Hint -> HighlightSeverity.INFORMATION
                        else -> HighlightSeverity.WARNING
                    }
                }
            }

            override fun getTooltip(diagnostic: Diagnostic): String {
                val code = diagnostic.code?.get()?.toString()
                val prefix = if (code != null) "<b>[$code]</b> " else ""
                // Nera puts label and hint on separate lines in the message
                val html = diagnostic.message
                    .replace("&", "&amp;")
                    .replace("<", "&lt;")
                    .replace(">", "&gt;")
                    .replace("\n", "<br>")
                return "$prefix$html"
            }

            override fun createAnnotation(
                holder: AnnotationHolder,
                diagnostic: Diagnostic,
                textRange: TextRange,
                quickFixes: List<IntentionAction>,
            ) {
                // Let the platform create the normal editor annotation.
                super.createAnnotation(holder, diagnostic, textRange, quickFixes)

                // Also mirror the diagnostic into our cache so that
                // NeraLspInspection can surface it in "Project Errors".
                val file = holder.currentAnnotationSession.file
                val fileUrl = file.virtualFile?.url ?: return
                val cache = project.getService(NeraLspDiagnosticCache::class.java)

                // Detect a new annotation session (= new file batch) and
                // start a new pending batch.  The live store is untouched
                // so NeraLspInspection always sees a complete previous batch.
                val session = holder.currentAnnotationSession
                if (lastSession?.get() !== session) {
                    cache.startBatch(fileUrl)
                    lastSession = WeakReference(session)
                }

                cache.add(
                    fileUrl,
                    NeraLspDiagnosticCache.Entry(
                        startOffset = textRange.startOffset,
                        endOffset   = textRange.endOffset,
                        severity    = getHighlightSeverity(diagnostic),
                        message     = diagnostic.message,
                        tooltip     = getTooltip(diagnostic),
                    ),
                )

                // Schedule a deferred check: after all createAnnotation calls
                // for this batch complete, commit the pending batch into the
                // live store and compare the content hash with the previous
                // known hash.  If changed, restart the daemon so
                // NeraLspInspection re-runs and populates "Project Errors".
                // On the second pass the hash will match -> no restart -> no loop.
                val virtualFile = file.virtualFile ?: return
                if (pendingFiles.add(fileUrl)) {
                    ApplicationManager.getApplication().invokeLater {
                        pendingFiles.remove(fileUrl)
                        if (project.isDisposed) return@invokeLater

                        // Compute hash from the pending batch before committing.
                        val newHash = cache.getPendingHash(fileUrl)
                        // Atomically swap pending batch into the live store.
                        cache.commitBatch(fileUrl)

                        val oldHash = lastKnownHash.put(fileUrl, newHash)
                        if (oldHash == null || oldHash != newHash) {
                            val psiFile = PsiManager.getInstance(project).findFile(virtualFile)
                            if (psiFile != null) {
                                DaemonCodeAnalyzer.getInstance(project).restart(psiFile)
                            }
                        }

                        // Report to WolfTheProblemSolver so files with problems
                        // are highlighted in the Project View and tracked in
                        // "Project Errors".  Wolf requires a background thread.
                        val entries = cache.get(fileUrl)
                        val source = this
                        ApplicationManager.getApplication().executeOnPooledThread {
                            if (project.isDisposed) return@executeOnPooledThread
                            val wolf = WolfTheProblemSolver.getInstance(project)
                            if (!entries.isNullOrEmpty()) {
                                wolf.reportProblemsFromExternalSource(virtualFile, source)
                            } else {
                                wolf.clearProblemsFromExternalSource(virtualFile, source)
                            }
                        }
                    }
                }
            }
        }

        override val semanticTokensCustomizer: LspSemanticTokensCustomizer = object : LspSemanticTokensSupport() {
            override fun shouldAskServerForSemanticTokens(psiFile: com.intellij.psi.PsiFile): Boolean = true

            override fun getTextAttributesKey(tokenType: String, modifiers: List<String>): TextAttributesKey? {
                return when (tokenType) {
                    // Function names — custom key with explicit blue color via colorSchemes/*.xml
                    "function" -> ErlangSyntaxHighlighter.FUNCTION_KEY
                    // Erlang macros (?MODULE, ?MY_MACRO) — use CONSTANT for a distinct color
                    "macro" -> DefaultLanguageHighlighterColors.CONSTANT
                    // Record field names — use INSTANCE_FIELD for property-like rendering
                    "property" -> DefaultLanguageHighlighterColors.INSTANCE_FIELD
                    // Module names in remote calls (io:format) — use CLASS_NAME instead of plain IDENTIFIER
                    "namespace" -> DefaultLanguageHighlighterColors.CLASS_NAME
                    // Type names in -spec, -type, -opaque, -callback — custom key with teal/cyan color
                    "type" -> ErlangSyntaxHighlighter.TYPE_KEY
                    // Variables — distinguish fresh bindings from bound variables (used for matching)
                    "variable" -> if ("definition" in modifiers) {
                        ErlangSyntaxHighlighter.VARIABLE_KEY
                    } else {
                        ErlangSyntaxHighlighter.BOUND_VARIABLE_KEY
                    }
                    else -> super.getTextAttributesKey(tokenType, modifiers)
                }
            }
        }
    }

    override fun isSupportedFile(file: VirtualFile): Boolean {
        return file.extension in setOf("erl", "hrl", "xrl", "yrl", "escript", "app") ||
               file.name.endsWith(".app.src")
    }

    override fun createCommandLine(): GeneralCommandLine {
        val configured = NeraSettings.getInstance().neraPath
        val binary = configured.ifBlank { System.getenv("NERA_PATH") ?: "nera" }
        val cmd = GeneralCommandLine(binary, "lsp", "--transport", "stdio")
        project.basePath?.let { cmd.withWorkDirectory(it) }
        return cmd
    }
}
