package dev.nekomario.amb

import java.io.InputStream

/** The same host-control reader/dispatcher serves USB and LAN. Camera actions
 * are delivered to the UI-thread owner; the reader never starts Camera2. */
class SessionControl(
    private val outbound: FramedOutbound,
    private val onQuality: (Wire.HostCommand) -> Unit,
    private val onPing: () -> Unit = {},
) {
    private var pingWindowStart = System.nanoTime()
    private var pingCount = 0
    private var idrWindowStart = System.nanoTime()
    private var idrCount = 0

    fun readAndDispatch(input: InputStream) {
        val (header, payload) = Wire.readHostFrame(input)
        dispatch(header, payload)
    }

    fun dispatch(header: Wire.Header, payload: ByteArray) {
        when (header.type) {
            Wire.TYPE_PING -> {
                val now = System.nanoTime()
                if (now - pingWindowStart >= 1_000_000_000L) {
                    pingWindowStart = now
                    pingCount = 0
                }
                require(++pingCount <= 64) { "PING rate limit exceeded" }
                outbound.sendPong(header, payload)
                onPing()
            }
            Wire.TYPE_VIDEO_IDR_REQUEST -> {
                // Same shape as the PING limit above, and measured rather than assumed:
                // an authorized PC could otherwise ask for an intra frame as fast as the
                // session accepts messages, and every request reaches
                // MediaCodec.setParameters(REQUEST_SYNC_FRAME) — the asymmetry with PING
                // was the finding. Extra requests inside the window are *dropped*, not
                // errors: one recovery is already pending and a keyframe is coming.
                val now = System.nanoTime()
                if (now - idrWindowStart >= 1_000_000_000L) {
                    idrWindowStart = now
                    idrCount = 0
                }
                if (++idrCount <= MAX_IDR_REQUESTS_PER_SECOND) outbound.forceRecovery()
            }
            Wire.TYPE_PREPARE_QUALITY -> onQuality(Wire.HostCommand.Prepare(Wire.decodePrepareQuality(payload)))
            Wire.TYPE_COMMIT_QUALITY -> onQuality(Wire.HostCommand.Commit(Wire.decodeEpoch(payload)))
            else -> error("unexpected authenticated host command")
        }
    }

    private companion object {
        /**
         * How many IDR requests one second may deliver.
         *
         * Not zero: a PC whose stream stalled legitimately needs a keyframe, and the
         * request path already coalesces (a drop while `recovering` is cleared by the
         * next keyframe). Not unbounded: the call reaches the hardware encoder.
         */
        const val MAX_IDR_REQUESTS_PER_SECOND = 8
    }
}
