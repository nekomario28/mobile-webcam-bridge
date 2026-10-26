package dev.nekomario.amb

/**
 * Time-to-first-frame marks. A phase name and a duration, nothing else.
 *
 * The line is safe to log: it cannot carry an address, a name, a key, or a
 * free-form error. [originMs] is a monotonic clock (elapsed realtime), not
 * wall time, so the number is not a timestamp of a person.
 */
internal object Ttff {
    const val TAG = "MobileWebcamTtff"

    private val phasePattern = Regex("[a-z0-9_]{1,32}")

    fun line(phase: String, originMs: Long, nowMs: Long): String {
        require(phasePattern.matches(phase)) { "ttff phase must be a short token" }
        val elapsed = (nowMs - originMs).coerceAtLeast(0)
        return "ttff phase=$phase elapsed_ms=$elapsed"
    }
}
