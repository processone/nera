package net.processone.nera

import com.intellij.ide.IconProvider
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import javax.swing.Icon

class ErlangIconProvider : IconProvider() {
    override fun getIcon(element: PsiElement, flags: Int): Icon? {
        if (element !is PsiFile) return null
        return when (element.virtualFile?.extension) {
            "erl" -> NeraIcons.ERLANG_MODULE
            "hrl" -> NeraIcons.ERLANG_HEADER
            "xrl", "yrl", "escript" -> NeraIcons.ERLANG_FILE
            else -> null
        }
    }
}
