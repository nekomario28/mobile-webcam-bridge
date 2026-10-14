package dev.nekomario.amb

import org.junit.Assert.assertEquals

/** Strict fixture parser: malformed lines fail, rather than disappearing from coverage. */
object QualityFixture {
    private fun reason(word: String) = when (word.removePrefix("reject-")) {
        "none" -> 0
        "permission" -> 1
        "foreground" -> 2
        "unsupported-mode" -> 3
        "stale-revision" -> 4
        "busy" -> 5
        "deadline" -> 6
        else -> error("unknown reason: $word")
    }
    fun run(script: String): List<List<QualitySwitch.Action>> {
        val machine = QualitySwitch.Machine()
        var now = 0L
        return script.lines().mapNotNull { raw ->
            val line = raw.substringBefore('#').trim()
            if (line.isEmpty()) return@mapNotNull null
            val halves = line.split("->")
            require(halves.size == 2) { "invalid fixture: $raw" }
            val words = halves[0].trim().split(Regex("\\s+"))
            fun n(index: Int) = words[index].toInt()
            val event = when (words[0]) {
                "prepare" -> QualitySwitch.Event.Prepare(words[1].toLong(), n(2), n(3), n(4), n(5),
                    if (words[6] == "accept") QualitySwitch.Verdict.Accept else QualitySwitch.Verdict.Reject(reason(words[6])),
                    words[7] == "streaming")
                "commit" -> QualitySwitch.Event.Commit(words[1].toLong(), words[2].toLong())
                "capture-started" -> QualitySwitch.Event.CaptureStarted(n(1), n(2), n(3))
                "deadline-exceeded" -> QualitySwitch.Event.DeadlineExceeded(words[1].toLong()).also { now = it.nowMs }
                "user-stop" -> QualitySwitch.Event.UserStop
                else -> error("unknown event: $raw")
            }
            val expected = halves[1].split(';').mapNotNull { text ->
                val a = text.trim().split(Regex("\\s+"))
                fun i(index: Int) = a[index].toInt()
                when (a[0]) {
                    "-" -> null
                    "send-paused" -> QualitySwitch.Action.SendPaused(a[1].toLong(), a[2].toLong())
                    "start-capture" -> QualitySwitch.Action.StartCapture(i(1), i(2), i(3))
                    "stop-capture" -> QualitySwitch.Action.StopCapture
                    "send-quality-result" -> QualitySwitch.Action.SendQualityResult(a[1].toLong(),
                        when (a[2]) { "switching" -> 0; "rejected" -> 1; "applied" -> 2; else -> error("unknown status: $raw") },
                        reason(a[3]), i(4), i(5), i(6))
                    else -> error("unknown action: $raw")
                }
            }
            machine.step(now, event).also { assertEquals("fixture line: $raw", expected, it) }
        }
    }
}
