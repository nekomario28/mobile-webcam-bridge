package dev.nekomario.amb

/**
 * Process-wide epoch of the authorization store.
 *
 * A deliberate reset must not be undone by a writer that was created before it.
 * Measured on the emulator: the reset returned without an error, and the record
 * and its key were both present again immediately afterwards — a live session's
 * expiry or revocation had re-created exactly what the user erased, and the next
 * read then failed with "record missing but its key remains", i.e. the recovery
 * action looked impossible.
 *
 * So every writer captures the epoch it was created in and may only publish while
 * that epoch is current. A stale writer fails closed instead of resurrecting the
 * data: its repository latches shut, and the app asks for a reconnect.
 */
internal object StorageGeneration {
    @Volatile private var value = 0L

    fun current(): Long = value

    /** Called by `ClientStore.reset()` before it clears anything. */
    @Synchronized fun bump(): Long {
        value += 1
        return value
    }

    fun requireCurrent(born: Long) {
        check(value == born) { "authorization storage was reset; this writer must reconnect" }
    }
}
