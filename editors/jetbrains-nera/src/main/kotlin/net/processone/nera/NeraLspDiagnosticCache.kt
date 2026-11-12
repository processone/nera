package net.processone.nera

import com.intellij.lang.annotation.HighlightSeverity
import com.intellij.openapi.components.Service
import com.intellij.openapi.project.Project
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.CopyOnWriteArrayList

/**
 * Project-level service that caches LSP diagnostics so they can be
 * served to our [NeraLspInspection].  The JetBrains platform shows
 * LSP push-diagnostics in the **File** problems tab but not in
 * **Project Errors** — that panel is fed exclusively by the inspection
 * framework.  This cache bridges the two.
 *
 * Uses double-buffering: new diagnostics are written to [pendingStore]
 * while [NeraLspInspection] reads from [store].  A [commitBatch] call
 * atomically swaps the pending list into the live store so the
 * inspection always sees a complete batch, never a partially-cleared one.
 *
 * Population happens in [NeraLspServerDescriptor]'s `createAnnotation`
 * override which is called by the platform for every LSP diagnostic.
 */
@Service(Service.Level.PROJECT)
class NeraLspDiagnosticCache(
    @Suppress("unused") private val project: Project,
) {
    /** One diagnostic entry ready for the inspection to consume. */
    data class Entry(
        val startOffset: Int,
        val endOffset: Int,
        val severity: HighlightSeverity,
        val message: String,
        val tooltip: String,
    )

    /* Live store: file-url → list of diagnostics.  Read by NeraLspInspection. */
    private val store = ConcurrentHashMap<String, CopyOnWriteArrayList<Entry>>()

    /* Pending store: accumulates diagnostics during an annotation batch. */
    private val pendingStore = ConcurrentHashMap<String, CopyOnWriteArrayList<Entry>>()

    /** Begin a new pending batch for [fileUrl]. */
    fun startBatch(fileUrl: String) {
        pendingStore[fileUrl] = CopyOnWriteArrayList()
    }

    /** Append a single diagnostic to the pending batch of [fileUrl]. */
    fun add(fileUrl: String, entry: Entry) {
        pendingStore.getOrPut(fileUrl) { CopyOnWriteArrayList() }.add(entry)
    }

    /**
     * Atomically swap the pending batch for [fileUrl] into the live store.
     * After this call, [get] returns the newly committed diagnostics.
     */
    fun commitBatch(fileUrl: String) {
        val pending = pendingStore.remove(fileUrl)
        if (pending != null) {
            store[fileUrl] = pending
        }
    }

    /** Return the committed diagnostics for [fileUrl], or `null`. */
    fun get(fileUrl: String): List<Entry>? = store[fileUrl]

    /** Return a content hash for [fileUrl] (from the pending batch) for change detection. */
    fun getPendingHash(fileUrl: String): Int = pendingStore[fileUrl]?.hashCode() ?: 0

    /** Remove all entries for [fileUrl]. */
    fun clearFile(fileUrl: String) {
        store.remove(fileUrl)
        pendingStore.remove(fileUrl)
    }
}
