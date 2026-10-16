package dev.nekomario.amb

import java.util.concurrent.Executors

/**
 * Off-UI producer teardown for authorization loss.
 *
 * The defect this exists to prevent (AUTHREV-001): the Camera2/MediaCodec
 * producer was stopped only from a callback that the Activity posted to the main
 * looper, so a busy main thread left the camera capturing after authorization
 * ended. The dispatcher therefore is not a parameter — this class owns one
 * dedicated daemon thread, so no caller can route the teardown through the main
 * looper, and [cancel] never runs the close on the calling thread (which is
 * normally the authorization timer or a transport reader).
 *
 * The Activity must hold one of these as a field
 * (`CaptureCancellationWiringTest` pins that) instead of posting its own stop.
 */
class CaptureCancellation(private val detach: () -> AutoCloseable?) {
    private val worker = Executors.newSingleThreadExecutor { job ->
        Thread(job, "mwb-capture-teardown").apply { isDaemon = true }
    }

    /**
     * Hands the teardown to the dedicated thread. [owns] is re-evaluated there,
     * so a session that has already been replaced is not torn down by a stale
     * cancellation. Never throws into authorization teardown: during shutdown the
     * Activity's own teardown path owns the last one.
     */
    fun cancel(owns: () -> Boolean = { true }) {
        runCatching {
            worker.execute {
                if (!owns()) return@execute
                detach()?.let { runCatching { it.close() } }
            }
        }
    }

    /** Idempotent; the Activity calls it from onDestroy. */
    fun shutdown() {
        worker.shutdownNow()
    }
}
