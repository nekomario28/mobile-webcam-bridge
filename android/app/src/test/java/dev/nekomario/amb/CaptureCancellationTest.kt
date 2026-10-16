package dev.nekomario.amb

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference

/**
 * Pins that authorization loss reaches the producer off the calling thread.
 *
 * AUTHREV-001 was that the producer stop was posted to the main looper, so a
 * busy main thread kept the camera capturing. The closest check without a device
 * is that the teardown completes while the *calling* thread stays busy: a stop
 * routed through the caller (or the main looper) cannot do that.
 */
class CaptureCancellationTest {
    private fun closer(latch: CountDownLatch, thread: AtomicReference<String>? = null) =
        object : AutoCloseable {
            override fun close() {
                thread?.set(Thread.currentThread().name)
                latch.countDown()
            }
        }

    @Test fun teardown_completes_while_the_calling_thread_stays_busy() {
        val closed = CountDownLatch(1)
        val callerReturned = CountDownLatch(1)
        val cancellation = CaptureCancellation { closer(closed) }
        try {
            val caller = Thread({
                cancellation.cancel()
                callerReturned.countDown()
                // Stay busy well past the assertion window: the teardown must not
                // wait for this thread, which is what a main-looper post would do.
                Thread.sleep(3_000)
            }, "busy-caller")
            caller.isDaemon = true
            caller.start()
            assertTrue("cancel() did not return promptly", callerReturned.await(1, TimeUnit.SECONDS))
            assertTrue("teardown waited for the busy calling thread",
                closed.await(1, TimeUnit.SECONDS))
        } finally {
            cancellation.shutdown()
        }
    }

    @Test fun teardown_runs_on_the_dedicated_thread_and_only_once() {
        val closed = CountDownLatch(1)
        val thread = AtomicReference<String>()
        val detaches = AtomicInteger()
        val cancellation = CaptureCancellation { detaches.incrementAndGet(); closer(closed, thread) }
        try {
            cancellation.cancel()
            assertTrue(closed.await(5, TimeUnit.SECONDS))
            assertNotEquals("teardown ran on the caller", Thread.currentThread().name, thread.get())
            assertEquals("mwb-capture-teardown", thread.get())
            assertEquals(1, detaches.get())
        } finally {
            cancellation.shutdown()
        }
    }

    @Test fun a_replaced_session_is_not_torn_down_by_a_stale_cancellation() {
        val closed = CountDownLatch(1)
        val detaches = AtomicInteger()
        val cancellation = CaptureCancellation { detaches.incrementAndGet(); closer(closed) }
        try {
            cancellation.cancel { false }
            Thread.sleep(300)
            assertEquals(0, detaches.get())
            assertEquals(1L, closed.count)
            cancellation.cancel { true }
            assertTrue(closed.await(5, TimeUnit.SECONDS))
            assertEquals(1, detaches.get())
        } finally {
            cancellation.shutdown()
        }
    }

    @Test fun a_failing_teardown_does_not_break_later_cancellations() {
        val closed = CountDownLatch(1)
        val attempts = AtomicInteger()
        val cancellation = CaptureCancellation {
            if (attempts.incrementAndGet() == 1) throw IllegalStateException("camera already gone")
            closer(closed)
        }
        try {
            cancellation.cancel()
            cancellation.cancel()
            assertTrue("a failing teardown stopped later cancellations",
                closed.await(5, TimeUnit.SECONDS))
            assertEquals(2, attempts.get())
        } finally {
            cancellation.shutdown()
        }
    }

    @Test fun cancel_is_safe_after_shutdown() {
        val cancellation = CaptureCancellation { null }
        cancellation.shutdown()
        cancellation.cancel()
    }
}
