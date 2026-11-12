package net.processone.nera

import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.platform.lsp.api.LspServerSupportProvider
import com.intellij.platform.lsp.api.lsWidget.LspServerWidgetItem

class NeraLspServerSupportProvider : LspServerSupportProvider {

    override fun fileOpened(
        project: Project,
        file: VirtualFile,
        serverStarter: LspServerSupportProvider.LspServerStarter
    ) {
        if (file.extension in setOf("erl", "hrl", "xrl", "yrl", "escript", "app") ||
            file.name.endsWith(".app.src")) {
            serverStarter.ensureServerStarted(NeraLspServerDescriptor(project))
        }
    }

    override fun createLspServerWidgetItem(lspServer: com.intellij.platform.lsp.api.LspServer, currentFile: VirtualFile?): LspServerWidgetItem =
        LspServerWidgetItem(lspServer, currentFile, settingsPageClass = NeraSettingsConfigurable::class.java)
}
