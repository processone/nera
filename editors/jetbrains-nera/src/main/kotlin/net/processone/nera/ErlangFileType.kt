package net.processone.nera

import com.intellij.openapi.fileTypes.LanguageFileType
import javax.swing.Icon

class ErlangFileType private constructor() : LanguageFileType(ErlangLanguage) {
    companion object {
        @JvmField val INSTANCE = ErlangFileType()
    }

    override fun getName() = "Erlang"
    override fun getDescription() = "Erlang source file"
    override fun getDefaultExtension() = "erl"
    override fun getIcon(): Icon = NeraIcons.ERLANG_FILE
}
