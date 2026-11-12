package net.processone.nera

import com.intellij.lang.Commenter

class ErlangCommenter : Commenter {
    override fun getLineCommentPrefix(): String = "% "

    override fun getLineCommentPrefixes(): MutableList<String> =
        mutableListOf("%%% ", "%% ", "% ")

    override fun getBlockCommentPrefix(): String? = null
    override fun getBlockCommentSuffix(): String? = null
    override fun getCommentedBlockCommentPrefix(): String? = null
    override fun getCommentedBlockCommentSuffix(): String? = null
}
