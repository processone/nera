package net.processone.nera

import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.lang.Language
import com.intellij.psi.codeStyle.lineIndent.LineIndentProvider

class ErlangLineIndentProvider : LineIndentProvider {

    override fun getLineIndent(project: Project, editor: Editor, language: Language?, offset: Int): String? {
        if (!isSuitableFor(language)) return null

        val document = editor.document
        val lineNum = document.getLineNumber(offset)
        if (lineNum < 0) return null

        val calculator = ErlangIndentCalculator(document)
        val indent = calculator.computeIndent(lineNum)
        return " ".repeat(indent)
    }

    override fun isSuitableFor(language: Language?): Boolean =
        language == ErlangLanguage
}
