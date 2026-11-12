package net.processone.nera

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.components.PersistentStateComponent
import com.intellij.openapi.components.Service
import com.intellij.openapi.components.State
import com.intellij.openapi.components.Storage

@Service(Service.Level.APP)
@State(name = "NeraSettings", storages = [Storage("nera.xml")])
class NeraSettings : PersistentStateComponent<NeraSettings.State> {
    data class State(var neraPath: String = "nera")

    private var myState = State()

    override fun getState(): State = myState

    override fun loadState(state: State) {
        myState = state
    }

    var neraPath: String
        get() = myState.neraPath
        set(value) { myState.neraPath = value }

    companion object {
        fun getInstance(): NeraSettings =
            ApplicationManager.getApplication().getService(NeraSettings::class.java)
    }
}
