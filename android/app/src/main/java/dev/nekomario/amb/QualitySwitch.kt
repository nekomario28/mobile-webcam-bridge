package dev.nekomario.amb

/**
 * The phone-side quality-switch controller: a pure event machine that owns
 * the capture owner and the uncommitted prepared state. Every transition
 * emits actions; the fixture script drives both implementations with the
 * same events and the same expected action trace.
 *
 * The apply contract (specs/video-quality-controls/slices/02-control.md):
 * prepare -> validate (permission, foreground, supported mode, revision) ->
 * rejected keeps streaming and only answers; accepted while streaming closes
 * capture and returns Paused; the PC's CommitQuality starts the new mode and
 * the applied result follows. One apply at a time; a repeated prepare or a
 * stale commit is ignored, never double-stopped. The uncommitted prepared
 * state has a 5-second deadline: on expiry the capture stays stopped and the
 * staged mode is released — never a silent rollback.
 */
object QualitySwitch {
    /** The uncommitted prepared state is bounded; a staged (idle) mode waits
     *  for the user's camera start instead. */
    const val PREPARED_DEADLINE_MS = 5_000L

    enum class Phase { Idle, Prepared, Starting, Applied, Failed }

    /** Why a prepare was rejected; matches the wire reason codes. */
    const val REASON_PERMISSION = 1
    const val REASON_FOREGROUND = 2
    const val REASON_UNSUPPORTED_MODE = 3
    const val REASON_STALE_REVISION = 4

    /** The verdict the caller computes from Android state and the catalog. */
    sealed interface Verdict {
        object Accept : Verdict
        data class Reject(val reason: Int) : Verdict
    }

    sealed interface Event {
        data class Prepare(
            val requestId: Int,
            val revision: Int,
            val width: Int,
            val height: Int,
            val fps: Int,
            val verdict: Verdict,
            val streaming: Boolean,
        ) : Event

        data class Commit(val requestId: Int, val epoch: Int) : Event
        data class CaptureStarted(val width: Int, val height: Int) : Event
        data class DeadlineExceeded(val nowMs: Long) : Event
        object UserStop : Event
    }

    sealed interface Action {
        data class SendQualityResult(
            val requestId: Int,
            val status: Int,
            val reason: Int,
            val width: Int,
            val height: Int,
            val fps: Int,
        ) : Action

        data class SendPaused(val requestId: Int, val epoch: Int) : Action
        data class StartCapture(val width: Int, val height: Int, val fps: Int) : Action
        object StopCapture : Action
    }

    class Machine(startingEpoch: Int = 40) {
        var phase: Phase = Phase.Idle
            private set
        /** The mode staged for the user's next camera start, if any. */
        var stagedWidth: Int = 0
            private set
        var stagedHeight: Int = 0
            private set
        var stagedFps: Int = 0
            private set

        private var epoch = startingEpoch
        private var lastRequestId = 0
        private var deadlineMs: Long? = null

        fun step(nowMs: Long, event: Event): List<Action> = when (event) {
            is Event.Prepare -> prepare(nowMs, event)
            is Event.Commit -> commit(event)
            is Event.CaptureStarted -> captureStarted(event)
            is Event.DeadlineExceeded -> deadline(nowMs, event)
            Event.UserStop -> userStop()
        }

        private fun prepare(nowMs: Long, event: Event.Prepare): List<Action> {
            // A repeated or stale prepare never acts twice.
            if (event.requestId <= lastRequestId) return emptyList()
            lastRequestId = event.requestId
            val reject = event.verdict as? Verdict.Reject
            if (reject != null) {
                // Rejection keeps the streaming mode; only an answer goes out.
                return listOf(
                    Action.SendQualityResult(
                        event.requestId,
                        Wire.RESULT_REJECTED,
                        reject.reason,
                        0,
                        0,
                        0,
                    ),
                )
            }
            // The accepted mode is staged in both cases: the streaming switch
            // starts it on CommitQuality, the idle case on the user's start.
            stagedWidth = event.width
            stagedHeight = event.height
            stagedFps = event.fps
            if (event.streaming) {
                // The boundary: capture closes; the uncommitted state starts.
                phase = Phase.Prepared
                deadlineMs = nowMs + PREPARED_DEADLINE_MS
                return listOf(Action.SendPaused(event.requestId, epoch))
            }
            // Nothing streams: the staged mode waits for the user's camera start.
            return listOf(
                Action.SendQualityResult(
                    event.requestId,
                    Wire.RESULT_SWITCHING,
                    0,
                    0,
                    0,
                    0,
                ),
            )
        }

        private fun commit(event: Event.Commit): List<Action> {
            if (phase != Phase.Prepared || event.requestId != lastRequestId) {
                return emptyList()
            }
            phase = Phase.Starting
            deadlineMs = null
            epoch = event.epoch
            return listOf(Action.StartCapture(stagedWidth, stagedHeight, stagedFps))
        }

        private fun captureStarted(event: Event.CaptureStarted): List<Action> {
            if (phase != Phase.Starting) return emptyList()
            phase = Phase.Applied
            return listOf(
                Action.SendQualityResult(
                    lastRequestId,
                    Wire.RESULT_APPLIED,
                    0,
                    event.width,
                    event.height,
                    stagedFps,
                ),
            )
        }

        private fun deadline(nowMs: Long, @Suppress("UNUSED_PARAMETER") event: Event.DeadlineExceeded): List<Action> {
            val expired = deadlineMs?.let { nowMs >= it } ?: false
            if (!expired || phase != Phase.Prepared) return emptyList()
            phase = Phase.Failed
            deadlineMs = null
            // Keep the capture stopped and release the staged mode; no rollback.
            stagedWidth = 0
            stagedHeight = 0
            stagedFps = 0
            return listOf(Action.StopCapture)
        }

        private fun userStop(): List<Action> {
            if (phase != Phase.Prepared) return emptyList()
            phase = Phase.Failed
            deadlineMs = null
            stagedWidth = 0
            stagedHeight = 0
            stagedFps = 0
            return listOf(Action.StopCapture)
        }

        // ---- Fixture script parsing (shared with the Rust implementation) ----

        companion object {
            fun parseFixtureLine(line: String): Pair<Event, List<Action>>? {
                val text = line.substringBefore('#').trim()
                if (text.isEmpty()) return null
                val (eventText, actionText) = text.split("->", limit = 2)
                val event = parseEvent(eventText.trim()) ?: return null
                val actions = actionText.split(';')
                    .map { it.trim() }
                    .filter { it.isNotEmpty() && it != "-" }
                    .mapNotNull { parseAction(it) }
                return event to actions
            }

            private fun parseEvent(text: String): Event? {
                val words = text.split(Regex("\\s+"))
                val numbers = words.drop(1).mapNotNull { it.toIntOrNull() }
                fun num(index: Int): Int? = numbers.getOrNull(index)
                return when (words.firstOrNull()) {
                    "prepare" -> Event.Prepare(
                        requestId = num(0) ?: return null,
                        revision = num(1) ?: return null,
                        width = num(2) ?: return null,
                        height = num(3) ?: return null,
                        fps = num(4) ?: return null,
                        verdict = verdictOf(words.getOrNull(5)),
                        streaming = words.getOrNull(6) == "streaming",
                    )

                    "commit" -> Event.Commit(num(0) ?: return null, num(1) ?: return null)
                    "capture-started" -> Event.CaptureStarted(num(0) ?: return null, num(1) ?: return null)
                    "deadline-exceeded" -> Event.DeadlineExceeded(words.getOrNull(1)?.toLongOrNull() ?: PREPARED_DEADLINE_MS)
                    "user-stop" -> Event.UserStop
                    else -> null
                }
            }

            private fun verdictOf(name: String?): Verdict = when (name) {
                "reject-permission" -> Verdict.Reject(REASON_PERMISSION)
                "reject-foreground" -> Verdict.Reject(REASON_FOREGROUND)
                "reject-mode" -> Verdict.Reject(REASON_UNSUPPORTED_MODE)
                "reject-revision" -> Verdict.Reject(REASON_STALE_REVISION)
                else -> Verdict.Accept
            }

            private fun parseAction(text: String): Action? {
                val words = text.split(Regex("\\s+"))
                val numbers = words.drop(1).mapNotNull { it.toIntOrNull() }
                fun num(index: Int): Int? = numbers.getOrNull(index)
                return when (words.firstOrNull()) {
                    "send-paused" -> Action.SendPaused(num(0) ?: return null, num(1) ?: return null)
                    "send-quality-result" -> Action.SendQualityResult(
                        num(0) ?: return null,
                        statusOf(words.getOrNull(1)),
                        reasonOf(words.getOrNull(2)),
                        num(3) ?: return null,
                        num(4) ?: return null,
                        num(5) ?: return null,
                    )

                    "start-capture" -> Action.StartCapture(num(0) ?: return null, num(1) ?: return null, num(2) ?: return null)
                    "stop-capture" -> Action.StopCapture
                    else -> null
                }
            }

            private fun statusOf(name: String?): Int = when (name) {
                "switching" -> Wire.RESULT_SWITCHING
                "rejected" -> Wire.RESULT_REJECTED
                else -> Wire.RESULT_APPLIED
            }

            private fun reasonOf(name: String?): Int = when (name) {
                "permission" -> REASON_PERMISSION
                "foreground" -> REASON_FOREGROUND
                "mode" -> REASON_UNSUPPORTED_MODE
                "revision" -> REASON_STALE_REVISION
                else -> 0
            }

            /** Run a fixture script against a fresh machine; a line mismatch
             *  throws with the offending line. */
            fun runFixture(script: String): List<List<Action>> {
                val machine = Machine()
                val trace = mutableListOf<List<Action>>()
                for (line in script.lines()) {
                    val (event, expected) = parseFixtureLine(line) ?: continue
                    val nowMs = if (line.contains("deadline-exceeded")) {
                        line.trim().split(Regex("\\s+")).getOrNull(1)?.toLongOrNull() ?: PREPARED_DEADLINE_MS
                    } else {
                        0L
                    }
                    val actions = machine.step(nowMs, event)
                    check(actions == expected) { "fixture line mismatch: $line (got $actions)" }
                    trace.add(actions)
                }
                return trace
            }
        }
    }
}
