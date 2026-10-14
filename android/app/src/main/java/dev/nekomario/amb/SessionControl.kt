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

    fun readAndDispatch(input: InputStream) {
        val (header, payload) = Wire.readHostFrame(input)
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
            Wire.TYPE_VIDEO_IDR_REQUEST -> outbound.forceRecovery()
            Wire.TYPE_PREPARE_QUALITY -> onQuality(Wire.HostCommand.Prepare(Wire.decodePrepareQuality(payload)))
            Wire.TYPE_COMMIT_QUALITY -> onQuality(Wire.HostCommand.Commit(Wire.decodeEpoch(payload)))
        }
    }
}
