package dev.nekomario.amb

/** Pure phone controller. The Activity owns capture; actions are executed on
 * its UI thread. Only replies are replayed: duplicate requests never capture. */
object QualitySwitch {
    const val PREPARED_DEADLINE_MS = 5_000L
    const val REASON_PERMISSION = 1
    const val REASON_FOREGROUND = 2
    const val REASON_UNSUPPORTED_MODE = 3
    const val REASON_STALE_REVISION = 4
    const val REASON_BUSY = 5
    const val REASON_DEADLINE = 6
    const val REASON_CAPTURE_FAILED = 7
    enum class Phase { Idle, Prepared, Staged, Starting, Applied, Failed }
    sealed interface Verdict {
        object Accept : Verdict
        data class Reject(val reason: Int) : Verdict
    }
    sealed interface Event {
        data class Prepare(val requestId: Long, val revision: Int, val width: Int, val height: Int,
            val fps: Int, val verdict: Verdict, val streaming: Boolean) : Event
        data class Commit(val requestId: Long, val epoch: Long) : Event
        data class CaptureStarted(val width: Int, val height: Int, val fps: Int) : Event
        data class DeadlineExceeded(val nowMs: Long) : Event
        object BeginStagedCapture : Event
        object CaptureFailed : Event
        object UserStop : Event
    }
    sealed interface Action {
        data class SendQualityResult(val requestId: Long, val status: Int, val reason: Int,
            val width: Int, val height: Int, val fps: Int) : Action
        data class SendPaused(val requestId: Long, val epoch: Long) : Action
        data class StartCapture(val width: Int, val height: Int, val fps: Int) : Action
        object StopCapture : Action
    }
    class Machine(startingEpoch: Long = 40) {
        var phase = Phase.Idle
            private set
        var stagedWidth = 0
            private set
        var stagedHeight = 0
            private set
        var stagedFps = 0
            private set
        var epoch = startingEpoch
            private set
        private var lastRequestId = 0L
        private var deadlineMs: Long? = null
        private var lastReply: List<Action> = emptyList()

        fun step(nowMs: Long, event: Event): List<Action> = when (event) {
            is Event.Prepare -> prepare(nowMs, event)
            is Event.Commit -> {
                if (phase != Phase.Prepared || event.requestId != lastRequestId ||
                    event.epoch != epoch + 1 || event.epoch > 0xffffffffL) emptyList()
                else {
                    phase = Phase.Starting
                    epoch = event.epoch
                    listOf(Action.StartCapture(stagedWidth, stagedHeight, stagedFps))
                }
            }
            is Event.CaptureStarted -> {
                if (phase != Phase.Starting && phase != Phase.Staged) emptyList()
                else if (event.width != stagedWidth || event.height != stagedHeight || event.fps != stagedFps) fail(REASON_UNSUPPORTED_MODE)
                else {
                    phase = Phase.Applied
                    deadlineMs = null
                    reply(Action.SendQualityResult(lastRequestId, Wire.RESULT_APPLIED, 0, event.width, event.height, event.fps))
                }
            }
            is Event.DeadlineExceeded -> {
                if (deadlineMs?.let { nowMs >= it } == true && (phase == Phase.Prepared || phase == Phase.Starting)) fail(REASON_DEADLINE)
                else emptyList()
            }
            Event.BeginStagedCapture -> {
                if (phase != Phase.Staged) emptyList()
                else {
                    phase = Phase.Starting
                    deadlineMs = nowMs + PREPARED_DEADLINE_MS
                    reply(Action.SendQualityResult(lastRequestId, Wire.RESULT_SWITCHING, 0, 0, 0, 0))
                }
            }
            Event.CaptureFailed -> {
                if (phase in listOf(Phase.Prepared, Phase.Staged, Phase.Starting)) fail(REASON_CAPTURE_FAILED) else emptyList()
            }
            Event.UserStop -> {
                if (phase == Phase.Prepared || phase == Phase.Staged || phase == Phase.Starting) fail(REASON_FOREGROUND)
                else emptyList()
            }
        }

        private fun prepare(nowMs: Long, event: Event.Prepare): List<Action> {
            if (event.requestId == lastRequestId && lastRequestId != 0L) return lastReply
            if (event.requestId !in 1..0xffffffffL || event.requestId < lastRequestId) return emptyList()
            if (phase == Phase.Prepared || phase == Phase.Starting || phase == Phase.Staged) {
                return listOf(Action.SendQualityResult(event.requestId, Wire.RESULT_REJECTED, REASON_BUSY, 0, 0, 0))
            }
            lastRequestId = event.requestId
            val reject = event.verdict as? Verdict.Reject
            if (reject != null) return reply(Action.SendQualityResult(event.requestId, Wire.RESULT_REJECTED, reject.reason, 0, 0, 0))
            stagedWidth = event.width
            stagedHeight = event.height
            stagedFps = event.fps
            if (event.streaming) {
                phase = Phase.Prepared
                deadlineMs = nowMs + PREPARED_DEADLINE_MS
                val paused = Action.SendPaused(event.requestId, epoch)
                lastReply = listOf(paused)
                return listOf(Action.StopCapture, paused)
            }
            phase = Phase.Staged
            deadlineMs = null
            return reply(Action.SendQualityResult(event.requestId, Wire.RESULT_STAGED, 0, 0, 0, 0))
        }

        private fun reply(action: Action): List<Action> = listOf(action).also { lastReply = it }
        private fun fail(reason: Int): List<Action> {
            phase = Phase.Failed
            deadlineMs = null
            stagedWidth = 0
            stagedHeight = 0
            stagedFps = 0
            return listOf(Action.StopCapture) + reply(Action.SendQualityResult(lastRequestId, Wire.RESULT_REJECTED, reason, 0, 0, 0))
        }
    }
}
