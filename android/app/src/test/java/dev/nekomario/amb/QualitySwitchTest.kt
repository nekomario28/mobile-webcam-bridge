package dev.nekomario.amb

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

/**
 * The phone-side switching state machine. The fixture script is the
 * observation entry: it drives the machine and asserts the exact action
 * trace, including the epoch advance, the rejection answer and the
 * uncommitted-state deadline.
 */
class QualitySwitchTest {
    private fun locateFixture(relative: String): File {
        var dir: File? = File("").absoluteFile
        repeat(5) {
            val candidate = File(dir, relative)
            if (candidate.isFile) return candidate
            dir = dir?.parentFile
        }
        throw AssertionError("fixture not found: $relative (searched up from ${File("").absolutePath})")
    }

    private fun prepare(
        requestId: Int,
        width: Int,
        height: Int,
        fps: Int,
        streaming: Boolean,
        verdict: QualitySwitch.Verdict = QualitySwitch.Verdict.Accept,
    ) = QualitySwitch.Event.Prepare(requestId.toLong(), 7, width, height, fps, verdict, streaming)

    @Test
    fun fixtureScriptDrivesTheExpectedActionTrace() {
        val script = locateFixture("protocol/fixtures/quality-switch-phone-v1.txt").readText()
        val trace = QualityFixture.run(script)
        assertTrue(trace.isNotEmpty())
    }

    @Test
    fun streamingSwitchPausesCommitsAndReportsApplied() {
        val machine = QualitySwitch.Machine()
        val paused = machine.step(0, prepare(1, 1280, 720, 30, streaming = true))
        assertEquals(listOf(QualitySwitch.Action.StopCapture, QualitySwitch.Action.SendPaused(1, 40)), paused)
        val started = machine.step(1, QualitySwitch.Event.Commit(1, 41))
        assertEquals(listOf(QualitySwitch.Action.StartCapture(1280, 720, 30)), started)
        val applied = machine.step(2, QualitySwitch.Event.CaptureStarted(1280, 720, 30))
        assertEquals(
            listOf(
                QualitySwitch.Action.SendQualityResult(1, Wire.RESULT_APPLIED, 0, 1280, 720, 30),
            ),
            applied,
        )
    }

    @Test
    fun pendingRequestCannotBeOverwrittenAndOldEpochCannotCommit() {
        val machine = QualitySwitch.Machine()
        machine.step(0, prepare(1, 1280, 720, 30, true))
        val busy = machine.step(1, prepare(2, 1920, 1080, 30, true))
        assertEquals(listOf(QualitySwitch.Action.SendQualityResult(2, Wire.RESULT_REJECTED, QualitySwitch.REASON_BUSY, 0, 0, 0)), busy)
        assertTrue(machine.step(2, QualitySwitch.Event.Commit(1, 40)).isEmpty())
        assertEquals(listOf(QualitySwitch.Action.StartCapture(1280, 720, 30)), machine.step(3, QualitySwitch.Event.Commit(1, 41)))
    }

    @Test
    fun wrongFpsAndBackgroundStageCannotBecomeApplied() {
        val machine = QualitySwitch.Machine()
        machine.step(0, prepare(1, 1280, 720, 30, false))
        val failed = machine.step(1, QualitySwitch.Event.CaptureStarted(1280, 720, 60))
        assertTrue(failed.contains(QualitySwitch.Action.StopCapture))
        assertEquals(QualitySwitch.Phase.Failed, machine.phase)
        machine.step(2, prepare(2, 1280, 720, 30, false))
        machine.step(3, QualitySwitch.Event.UserStop)
        assertEquals(0, machine.stagedWidth)
        assertTrue(machine.step(4, QualitySwitch.Event.CaptureStarted(1280, 720, 30)).isEmpty())
    }

    @Test
    fun rejectionKeepsStreamingAndOnlyAnswers() {
        val machine = QualitySwitch.Machine()
        val actions = machine.step(
            0,
            prepare(1, 1920, 1080, 30, streaming = true, verdict = QualitySwitch.Verdict.Reject(QualitySwitch.REASON_PERMISSION)),
        )
        assertEquals(
            listOf(
                QualitySwitch.Action.SendQualityResult(1, Wire.RESULT_REJECTED, QualitySwitch.REASON_PERMISSION, 0, 0, 0),
            ),
            actions,
        )
        assertEquals(QualitySwitch.Phase.Idle, machine.phase)
    }

    @Test
    fun repeatedPrepareAndStaleCommitNeverActTwice() {
        val machine = QualitySwitch.Machine()
        machine.step(0, prepare(1, 1280, 720, 30, streaming = true))
        // A duplicate replays its reply without stopping capture twice.
        assertEquals(listOf(QualitySwitch.Action.SendPaused(1, 40)), machine.step(1, prepare(1, 1280, 720, 30, streaming = true)))
        assertTrue(machine.step(1, QualitySwitch.Event.Commit(99, 41)).isEmpty())
        // The real commit starts capture exactly once.
        assertEquals(1, machine.step(2, QualitySwitch.Event.Commit(1, 41)).size)
        assertTrue(machine.step(3, QualitySwitch.Event.Commit(1, 41)).isEmpty())
    }

    @Test
    fun uncommittedPreparedStateExpiresByStoppingNotRollingBack() {
        val machine = QualitySwitch.Machine()
        machine.step(0, prepare(1, 1280, 720, 30, streaming = true))
        assertTrue(machine.step(4_000, QualitySwitch.Event.DeadlineExceeded(4_000)).isEmpty())
        val actions = machine.step(5_000, QualitySwitch.Event.DeadlineExceeded(5_000))
        assertEquals(listOf(QualitySwitch.Action.StopCapture, QualitySwitch.Action.SendQualityResult(1, Wire.RESULT_REJECTED, QualitySwitch.REASON_DEADLINE, 0, 0, 0)), actions)
        assertEquals(QualitySwitch.Phase.Failed, machine.phase)
        // The staged mode is released, not silently restored.
        assertEquals(0, machine.stagedWidth)
    }

    @Test
    fun idlePrepareStagesTheModeForTheUsersCameraStart() {
        val machine = QualitySwitch.Machine()
        val staged = machine.step(0, prepare(1, 1920, 1080, 30, streaming = false))
        assertEquals(
            listOf(QualitySwitch.Action.SendQualityResult(1, Wire.RESULT_SWITCHING, 0, 0, 0, 0)),
            staged,
        )
        assertEquals(1920, machine.stagedWidth)
        // No timer expires a human's wait, and the PC sends no commit here.
        assertTrue(machine.step(600_000, QualitySwitch.Event.DeadlineExceeded(600_000)).isEmpty())
        // The user starts the camera; the capture start completes the apply.
        val applied = machine.step(600_002, QualitySwitch.Event.CaptureStarted(1920, 1080, 30))
        assertEquals(
            listOf(
                QualitySwitch.Action.SendQualityResult(1, Wire.RESULT_APPLIED, 0, 1920, 1080, 30),
            ),
            applied,
        )
    }
}
