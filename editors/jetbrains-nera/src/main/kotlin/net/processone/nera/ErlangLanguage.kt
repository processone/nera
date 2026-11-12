package net.processone.nera

import com.intellij.lang.Language

object ErlangLanguage : Language("Erlang") {
    private fun readResolve(): Any = ErlangLanguage
}
