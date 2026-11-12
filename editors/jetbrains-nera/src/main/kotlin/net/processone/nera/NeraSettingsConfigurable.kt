package net.processone.nera

import com.intellij.openapi.fileChooser.FileChooserDescriptor
import com.intellij.openapi.options.Configurable
import com.intellij.openapi.ui.TextFieldWithBrowseButton
import com.intellij.util.ui.FormBuilder
import javax.swing.JComponent
import javax.swing.JPanel

class NeraSettingsConfigurable : Configurable {
    private var pathField: TextFieldWithBrowseButton? = null
    private var panel: JPanel? = null

    override fun getDisplayName(): String = "Nera"

    override fun createComponent(): JComponent {
        val descriptor = FileChooserDescriptor(true, false, false, false, false, false)
            .withTitle("Select Nera Binary")
            .withDescription("Path to the nera executable")
        val field = TextFieldWithBrowseButton()
        field.addBrowseFolderListener(null, descriptor)
        pathField = field
        panel = FormBuilder.createFormBuilder()
            .addLabeledComponent("Nera binary path:", field)
            .addComponentFillVertically(JPanel(), 0)
            .panel
        return panel!!
    }

    override fun isModified(): Boolean {
        return pathField?.text != NeraSettings.getInstance().neraPath
    }

    override fun apply() {
        NeraSettings.getInstance().neraPath = pathField?.text ?: "nera"
    }

    override fun reset() {
        pathField?.text = NeraSettings.getInstance().neraPath
    }

    override fun disposeUIResources() {
        pathField = null
        panel = null
    }
}
