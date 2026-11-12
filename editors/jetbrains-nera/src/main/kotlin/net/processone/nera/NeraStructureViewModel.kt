package net.processone.nera

import com.intellij.ide.structureView.StructureViewModel
import com.intellij.ide.structureView.StructureViewTreeElement
import com.intellij.ide.structureView.TextEditorBasedStructureViewModel
import com.intellij.ide.util.treeView.smartTree.Sorter
import com.intellij.openapi.editor.Editor
import com.intellij.psi.PsiFile

class NeraStructureViewModel(
    psiFile: PsiFile,
    editor: Editor?,
) : TextEditorBasedStructureViewModel(editor, psiFile),
    StructureViewModel.ElementInfoProvider {

    override fun getRoot() = NeraFileTreeElement(psiFile)

    override fun getSorters(): Array<Sorter> = arrayOf(Sorter.ALPHA_SORTER)

    override fun isAlwaysShowsPlus(element: StructureViewTreeElement): Boolean = false

    override fun isAlwaysLeaf(element: StructureViewTreeElement): Boolean = false
}
