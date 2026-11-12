package net.processone.nera

import com.intellij.lang.ASTNode
import com.intellij.lang.ParserDefinition
import com.intellij.lang.PsiParser
import com.intellij.lexer.Lexer
import com.intellij.openapi.project.Project
import com.intellij.psi.FileViewProvider
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.extapi.psi.PsiFileBase
import com.intellij.psi.tree.IFileElementType
import com.intellij.psi.tree.TokenSet

/**
 * Minimal ParserDefinition required for LanguageFileType registration.
 * Actual parsing/analysis is delegated to the LSP server.
 */
class ErlangParserDefinition : ParserDefinition {

    companion object {
        val FILE = IFileElementType(ErlangLanguage)
        val COMMENTS = TokenSet.create(ErlangTokenTypes.COMMENT)
        val STRINGS = TokenSet.create(
            ErlangTokenTypes.STRING,
            ErlangTokenTypes.TRIPLE_STRING,
            ErlangTokenTypes.QUOTED_ATOM,
            ErlangTokenTypes.CHAR_LITERAL
        )
    }

    override fun createLexer(project: Project?): Lexer = ErlangLexer()

    override fun createParser(project: Project?): PsiParser = PsiParser { root, builder ->
        val marker = builder.mark()
        while (!builder.eof()) builder.advanceLexer()
        marker.done(root)
        builder.treeBuilt
    }

    override fun getFileNodeType(): IFileElementType = FILE

    override fun getCommentTokens(): TokenSet = COMMENTS
    override fun getStringLiteralElements(): TokenSet = STRINGS

    override fun createElement(node: ASTNode): PsiElement =
        com.intellij.psi.impl.source.tree.LeafPsiElement(node.elementType, node.text)

    override fun createFile(viewProvider: FileViewProvider): PsiFile =
        object : PsiFileBase(viewProvider, ErlangLanguage) {
            override fun getFileType() = ErlangFileType.INSTANCE
        }
}
