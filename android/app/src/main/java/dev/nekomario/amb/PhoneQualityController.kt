package dev.nekomario.amb

/** UI-thread interpreter for the pure machine. Network sessions only deliver
 * commands. The actual camera and single writer remain their existing owners. */
class PhoneQualityController(
    private val session: MediaSession,
    private val revision: () -> Int,
    private val modes: () -> List<CaptureModes.CaptureMode>,
    private val permissionReady: () -> Boolean,
    private val foreground: () -> Boolean,
    private val streaming: () -> Boolean,
    private val stopCapture: () -> Unit,
    private val startCapture: (CaptureModes.CaptureMode) -> Unit,
    private val nowMillis: () -> Long = { System.nanoTime() / 1_000_000L },
    startingEpoch: Long = 0,
) {
    private val machine = QualitySwitch.Machine(startingEpoch)
    private var preparedMode: CaptureModes.CaptureMode? = null
    private var preparedId = 0L
    val epoch: Long get() = machine.epoch
    val stagedMode: CaptureModes.CaptureMode?
        get() = if (machine.phase == QualitySwitch.Phase.Staged) preparedMode else null
    val pending: Boolean get() = machine.phase in listOf(QualitySwitch.Phase.Prepared, QualitySwitch.Phase.Starting)

    fun handle(command: Wire.HostCommand) {
        val actions = when (command) {
            is Wire.HostCommand.Prepare -> {
                val request = command.request
                val mode = modes().firstOrNull { it.width == request.width && it.height == request.height && it.targetFps == request.targetFps }
                val reason = when {
                    !permissionReady() -> QualitySwitch.REASON_PERMISSION
                    !foreground() -> QualitySwitch.REASON_FOREGROUND
                    request.catalogRevision != revision() -> QualitySwitch.REASON_STALE_REVISION
                    mode == null -> QualitySwitch.REASON_UNSUPPORTED_MODE
                    else -> 0
                }
                val actions = machine.step(nowMillis(), QualitySwitch.Event.Prepare(
                    request.requestId, request.catalogRevision, request.width, request.height, request.targetFps,
                    if (reason == 0) QualitySwitch.Verdict.Accept else QualitySwitch.Verdict.Reject(reason), streaming(),
                ))
                if (request.requestId != preparedId && mode != null && actions.any {
                    it is QualitySwitch.Action.SendPaused || (it is QualitySwitch.Action.SendQualityResult && it.status == Wire.RESULT_SWITCHING)
                }) {
                    preparedId = request.requestId
                    preparedMode = mode
                }
                actions
            }
            is Wire.HostCommand.Commit -> {
                if (!foreground() || !permissionReady()) { cancel(); return }
                machine.step(nowMillis(), QualitySwitch.Event.Commit(command.epoch.requestId, command.epoch.streamEpoch))
            }
        }
        execute(actions)
    }

    fun captureStarted(width: Int, height: Int, fps: Int) = execute(machine.step(
        nowMillis(), QualitySwitch.Event.CaptureStarted(width, height, fps),
    ))

    fun tick() = execute(machine.step(nowMillis(), QualitySwitch.Event.DeadlineExceeded(nowMillis())))
    fun cancel() {
        execute(machine.step(nowMillis(), QualitySwitch.Event.UserStop))
        preparedMode = null
    }

    private fun execute(actions: List<QualitySwitch.Action>) {
        for (action in actions) when (action) {
            QualitySwitch.Action.StopCapture -> stopCapture()
            is QualitySwitch.Action.SendPaused -> session.pauseQuality(Wire.Epoch(action.requestId, action.epoch))
            is QualitySwitch.Action.StartCapture -> {
                val mode = requireNotNull(preparedMode) { "no prepared capture mode" }
                startCapture(mode)
            }
            is QualitySwitch.Action.SendQualityResult -> session.sendQualityResult(Wire.QualityResult(
                action.requestId, action.status, action.reason, action.width, action.height, action.fps,
            ))
        }
    }
}
