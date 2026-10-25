package dev.nekomario.amb

/**
 * The single-reader handover record, in one place.
 *
 * On USB the previous session's reader and a new session's reader share one endpoint, and a
 * blocked read is not interruptible — closing the descriptor is what unblocks it. When that
 * does not take effect the old reader keeps taking frames, which the phone and the host
 * report as `BAD_SIGNATURE` and "expected authorization outcome before capabilities/media",
 * both measured on a real phone, intermittently.
 *
 * The record holds a *liveness question about the session object*, not the answer or a
 * reference to the Activity's current-session field. `closeSession` clears that field before
 * it answers, so a guard that read the field held for exactly one iteration (independent
 * review, 2026-10-24, WR2 / ID-002), and a snapshot taken at record time would refuse for
 * ever after the reader returned.
 */
internal class ReaderHandover {
    private var stillReading: (() -> Boolean)? = null

    /**
     * Record the outcome of trying to close the current session.
     *
     * `stopped == true` leaves nothing behind — including when a session that is not an
     * accessory was closed at all, which is why the caller's default is "stopped". Otherwise
     * the liveness question is kept and asked again on every later attempt.
     */
    fun recordClose(stopped: Boolean, isReaderAlive: () -> Boolean) {
        stillReading = if (stopped) null else isReaderAlive
    }

    /** Whether the remembered reader is still alive, so opening another one must wait. */
    fun refuseWhileReading(): Boolean = stillReading?.invoke() == true

    /** Forget the record: nothing is holding the endpoint back. */
    fun clear() {
        stillReading = null
    }
}
