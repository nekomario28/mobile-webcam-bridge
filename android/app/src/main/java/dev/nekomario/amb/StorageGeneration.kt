package dev.nekomario.amb

/**
 * Epoch of one authorization namespace, bumped by a deliberate reset.
 *
 * A reset must not be undone by a writer that was created before it. Measured on
 * the emulator: the reset returned without an error, and the record and its key
 * were both present again immediately afterwards — a live session's expiry or
 * revocation had re-created exactly what the user erased, and the next read then
 * failed with "record missing but its key remains", i.e. the recovery action
 * looked impossible.
 *
 * So every writer captures the epoch of its own namespace and may only publish
 * while that epoch is current. A stale writer fails closed instead of
 * resurrecting the data: its repository latches shut, and the app asks for a
 * reconnect. Per namespace, not process-wide, so resetting one store cannot latch
 * a different one shut.
 */
internal object StorageGeneration {
    private val epochs = HashMap<String, Long>()

    fun current(namespace: String): Long = synchronized(epochs) { epochs[namespace] ?: 0L }

    /** Called by `ClientStore.clear` before it removes anything. */
    fun bump(namespace: String): Long = synchronized(epochs) {
        val next = (epochs[namespace] ?: 0L) + 1
        epochs[namespace] = next
        next
    }

    fun requireCurrent(namespace: String, born: Long) {
        check(current(namespace) == born) {
            "authorization storage was reset; this writer must reconnect"
        }
    }
}
