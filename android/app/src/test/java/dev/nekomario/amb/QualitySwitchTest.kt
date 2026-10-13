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
    ) = QualitySwitch.Event.Prepare(requestId, 7, width, height, fps, verdict, streaming)

    @Test
    fun fixtureScriptDrivesTheExpectedActionTrace() {
        val script = locateFixture("protocol/fixtures/quality-switch-phone-v1.txt").readText()
        val trace = QualitySwitch.Machine.runFixture(script)
        assertTrue(trace.isNotEmpty())
    }

    @Test
    fun streamingSwitchPausesCommitsAndReportsApplied() {
        val machine = QualitySwitch.Machine()
        val paused = machine.step(0, prepare(1, 1280, 720, 30, streaming = true))
        assertEquals(listOf(QualitySwitch.Action.SendPaused(1, 40)), paused)
        val started = machine.step(1, QualitySwitch.Event.Commit(1, 41))
        assertEquals(listOf(QualitySwitch.Action.StartCapture(1280, 720, 30)), started)
        val applied = machine.step(2, QualitySwitch.Event.CaptureStarted(1280, 720))
        assertEquals(
            listOf(
                QualitySwitch.Action.SendQualityResult(1, Wire.RESULT_APPLIED, 0, 1280, 720, 30),
            ),
            applied,
        )
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
        // The same request again, and a stale commit id: nothing happens.
        assertTrue(machine.step(1, prepare(1, 1280, 720, 30, streaming = true)).isEmpty())
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
        assertEquals(listOf(QualitySwitch.Action.StopCapture), actions)
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
        // No timer expires a human's wait.
        assertTrue(machine.step(600_000, QualitySwitch.Event.DeadlineExceeded(600_000)).isEmpty())
        // The user starts the camera; the commit completes the apply.
        machine.step(600_001, QualitySwitch.Event.Commit(1, 41))
        val applied = machine.step(600_002, QualitySwitch.Event.CaptureStarted(1920, 1080))
        assertEquals(
            listOf(
                QualitySwitch.Action.SendQualityResult(1, Wire.RESULT_APPLIED, 0, 1920, 1080, 30),
            ),
            applied,
        )
    }
}
