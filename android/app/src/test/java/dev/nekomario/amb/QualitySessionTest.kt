package dev.nekomario.amb

import org.junit.Assert.*
import org.junit.Test
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.OutputStream
import java.net.ServerSocket
import java.net.Socket
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger

class QualitySessionTest {
    private fun send(socket: Socket, type: Byte, payload: ByteArray) = Wire.writeFrame(
        socket.getOutputStream(), Wire.Header(type, 0, 1, payload.size, 0), payload,
    )

    @Test
    fun realLanReaderControllerAndWriterSwitchOnlyOnValidCommit() {
        ServerSocket(0).use { listener ->
            Socket("127.0.0.1", listener.localPort).use { client ->
                client.soTimeout = 2000
                val stops = AtomicInteger()
                val starts = AtomicInteger()
                val mode = CaptureModes.supported(CaptureModes.Capabilities("0", "fixture-avc",
                    listOf(CaptureModes.SizeFps(1920, 1080, null)), listOf(CaptureModes.FpsRange(30, 30)))).single()
                val caps = Wire.Capabilities(1, Wire.PERMISSION_READY, 7, 40, listOf(Wire.QualityMode(1920, 1080, 30, mode.bitrate)))
                lateinit var controller: PhoneQualityController
                val session = LanSession(listener.accept(), { _, _ -> }, { caps }, { _, command -> controller.handle(command) })
                controller = PhoneQualityController(session, { 7 }, { listOf(mode) }, { true }, { true }, { true },
                    stopCapture = { stops.incrementAndGet() },
                    startCapture = { actual ->
                        assertEquals(mode, actual)
                        starts.incrementAndGet()
                        controller.captureStarted(actual.width, actual.height, actual.targetFps)
                    }, startingEpoch = 40)
                try {
                    session.start()
                    assertEquals(Wire.TYPE_QUALITY_CAPABILITIES, Wire.readFrame(client.getInputStream()).first.type)
                    val prepare = Wire.encodePrepareQuality(Wire.PrepareQuality(1, 7, 1920, 1080, 30))
                    send(client, Wire.TYPE_PREPARE_QUALITY, prepare)
                    val paused = Wire.readFrame(client.getInputStream())
                    assertEquals(Wire.TYPE_PAUSED, paused.first.type)
                    assertEquals(Wire.Epoch(1, 40), Wire.decodeEpoch(paused.second))
                    assertEquals(1, stops.get())
                    assertEquals(0, starts.get())
                    send(client, Wire.TYPE_PREPARE_QUALITY, prepare)
                    assertEquals(Wire.TYPE_PAUSED, Wire.readFrame(client.getInputStream()).first.type)
                    assertEquals(1, stops.get())
                    send(client, Wire.TYPE_PREPARE_QUALITY, Wire.encodePrepareQuality(Wire.PrepareQuality(2, 7, 1920, 1080, 30)))
                    val busy = Wire.decodeQualityResult(Wire.readFrame(client.getInputStream()).second)
                    assertEquals(QualitySwitch.REASON_BUSY, busy.reason)
                    // Stale epoch must not start; a PONG confirms reader progress.
                    send(client, Wire.TYPE_COMMIT_QUALITY, Wire.encodeEpoch(Wire.Epoch(1, 42)))
                    send(client, Wire.TYPE_PING, ByteArray(8))
                    assertEquals(Wire.TYPE_PONG, Wire.readFrame(client.getInputStream()).first.type)
                    assertEquals(0, starts.get())
                    send(client, Wire.TYPE_COMMIT_QUALITY, Wire.encodeEpoch(Wire.Epoch(1, 41)))
                    val applied = Wire.decodeQualityResult(Wire.readFrame(client.getInputStream()).second)
                    assertEquals(Wire.QualityResult(1, Wire.RESULT_APPLIED, 0, 1920, 1080, 30), applied)
                    assertEquals(1, starts.get())
                    send(client, Wire.TYPE_COMMIT_QUALITY, Wire.encodeEpoch(Wire.Epoch(1, 41)))
                    send(client, Wire.TYPE_PING, ByteArray(8))
                    assertEquals(Wire.TYPE_PONG, Wire.readFrame(client.getInputStream()).first.type)
                    assertEquals(1, starts.get())
                } finally { session.close() }
            }
        }
    }

    @Test
    fun deniedPermissionAndIdleStagingNeverStartTheCameraFromPc() {
        ServerSocket(0).use { listener ->
            Socket("127.0.0.1", listener.localPort).use { client ->
                client.soTimeout = 2000
                val starts = AtomicInteger()
                var allowed = false
                var now = 0L
                val mode = CaptureModes.supported(CaptureModes.Capabilities("0", "fixture",
                    listOf(CaptureModes.SizeFps(1280, 720, null)), listOf(CaptureModes.FpsRange(30, 30)))).single()
                lateinit var controller: PhoneQualityController
                val session = LanSession(listener.accept(), { _, _ -> }, onQuality = { _, command -> controller.handle(command) })
                controller = PhoneQualityController(session, { 7 }, { listOf(mode) }, { allowed }, { true }, { false },
                    stopCapture = {}, startCapture = { starts.incrementAndGet() }, nowMillis = { now })
                try {
                    session.start()
                    send(client, Wire.TYPE_PREPARE_QUALITY, Wire.encodePrepareQuality(Wire.PrepareQuality(1, 7, 1280, 720, 30)))
                    assertEquals(QualitySwitch.REASON_PERMISSION, Wire.decodeQualityResult(Wire.readFrame(client.getInputStream()).second).reason)
                    assertEquals(0, starts.get())
                    allowed = true
                    send(client, Wire.TYPE_PREPARE_QUALITY, Wire.encodePrepareQuality(Wire.PrepareQuality(2, 7, 1280, 720, 30)))
                    assertEquals(Wire.RESULT_STAGED, Wire.decodeQualityResult(Wire.readFrame(client.getInputStream()).second).status)
                    now = 600000
                    controller.tick()
                    assertEquals(mode, controller.stagedMode)
                    assertEquals(0, starts.get())
                    // Only the phone user's start completes an idle stage.
                    controller.captureRequested()
                    assertEquals(Wire.RESULT_SWITCHING, Wire.decodeQualityResult(Wire.readFrame(client.getInputStream()).second).status)
                    controller.captureStarted(1280, 720, 30)
                    assertEquals(Wire.RESULT_APPLIED, Wire.decodeQualityResult(Wire.readFrame(client.getInputStream()).second).status)
                } finally { session.close() }
            }
        }
    }

    @Test
    fun pausedBarrierDropsQueuedAndLateOldGenerationVideo() {
        val blocked = CountDownLatch(1)
        val release = CountDownLatch(1)
        val flushed = CountDownLatch(7)
        val buffer = ByteArrayOutputStream()
        val output = object : OutputStream() {
            var first = true
            override fun write(value: Int) { buffer.write(value) }
            override fun write(bytes: ByteArray, offset: Int, length: Int) {
                if (first && length == Wire.HEADER_SIZE && bytes[offset + 5] == Wire.TYPE_VIDEO_AU) {
                    first = false; blocked.countDown(); check(release.await(3, TimeUnit.SECONDS))
                }
                buffer.write(bytes, offset, length)
            }
            override fun flush() { flushed.countDown() }
        }
        val writer = FramedOutbound(output, {}, { error(it) })
        fun unit(value: Byte) = H264Encoder.AccessUnit(byteArrayOf(value), value.toLong(), true, false)
        try {
            writer.start()
            val old = writer.beginCapture()
            writer.sendVideoConfig(64, 32, 30, 100000, old)
            writer.sendAccessUnit(unit(1), old)
            assertTrue(blocked.await(2, TimeUnit.SECONDS))
            writer.sendAccessUnit(unit(2), old)
            writer.pauseQuality(Wire.Epoch(1, 0))
            val fresh = writer.beginCapture()
            writer.sendVideoConfig(64, 32, 30, 100000, fresh)
            // A duplicate Prepare may replay PAUSED after Commit. It must
            // replay only the reply, not disable the new generation.
            writer.pauseQuality(Wire.Epoch(1, 0))
            writer.sendAccessUnit(unit(3), old)
            writer.sendAccessUnit(unit(4), fresh)
            writer.sendPong(Wire.Header(Wire.TYPE_PING, 0, 1, 8, 0), ByteArray(8))
            release.countDown()
            assertTrue(flushed.await(2, TimeUnit.SECONDS))
            val input = ByteArrayInputStream(buffer.toByteArray())
            val frames = buildList { while (input.available() > 0) add(Wire.readFrame(input)) }
            assertEquals(listOf(1.toByte(), 4.toByte()), frames.filter { it.first.type == Wire.TYPE_VIDEO_AU }.map { it.second.single() })
            assertEquals(Wire.TYPE_PAUSED, frames[2].first.type)
            assertTrue(frames.drop(3).none { it.first.type == Wire.TYPE_VIDEO_AU && it.second.single() != 4.toByte() })
        } finally { release.countDown(); writer.close() }
    }

    @Test
    fun stagedStartDeadlineClearsThePhoneStartIntentThroughTheRealWriter() {
        ServerSocket(0).use { listener ->
            Socket("127.0.0.1", listener.localPort).use { client ->
                client.soTimeout = 2000
                var now = 0L
                var cleared = false
                val mode = CaptureModes.supported(CaptureModes.Capabilities("0", "fixture",
                    listOf(CaptureModes.SizeFps(1280, 720, null)), listOf(CaptureModes.FpsRange(30, 30)))).single()
                lateinit var controller: PhoneQualityController
                val session = LanSession(listener.accept(), { _, _ -> }, onQuality = { _, command -> controller.handle(command) })
                controller = PhoneQualityController(session, { 7 }, { listOf(mode) }, { true }, { true }, { false },
                    stopCapture = { clear -> cleared = clear }, startCapture = { error("PC started idle camera") }, nowMillis = { now })
                try {
                    session.start()
                    send(client, Wire.TYPE_PREPARE_QUALITY, Wire.encodePrepareQuality(Wire.PrepareQuality(1, 7, 1280, 720, 30)))
                    assertEquals(Wire.RESULT_STAGED, Wire.decodeQualityResult(Wire.readFrame(client.getInputStream()).second).status)
                    now = 600000; controller.tick(); assertEquals(mode, controller.stagedMode)
                    controller.captureRequested()
                    assertEquals(Wire.RESULT_SWITCHING, Wire.decodeQualityResult(Wire.readFrame(client.getInputStream()).second).status)
                    now += 5000; controller.tick()
                    assertEquals(QualitySwitch.REASON_DEADLINE, Wire.decodeQualityResult(Wire.readFrame(client.getInputStream()).second).reason)
                    assertTrue(cleared)
                    assertEquals(null, controller.stagedMode)
                } finally { session.close() }
            }
        }
    }

    @Test
    fun hostTypeLimitsRejectBeforeReadingPayload() {
        val bytes = ByteArrayOutputStream()
        Wire.writeFrame(bytes, Wire.Header(Wire.TYPE_VIDEO_IDR_REQUEST, 0, 1, 14, 0), ByteArray(14))
        assertThrows(IllegalArgumentException::class.java) {
            Wire.readHostFrame(ByteArrayInputStream(bytes.toByteArray().copyOf(Wire.HEADER_SIZE)))
        }
    }
}
