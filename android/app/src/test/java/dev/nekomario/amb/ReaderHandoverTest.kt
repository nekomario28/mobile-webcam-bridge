package dev.nekomario.amb

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The single-reader refusal, driven as the property the substring test could not see.
 *
 * The review's point about the guard it replaced (`SessionServiceWiringTest`) is that three
 * assertions on source text were all present while the behaviour could still be absent: they
 * cannot see the `?: true` default, the rediscovery, or that the refusal path re-enters the
 * function it guards (independent review, 2026-10-24, WR2).
 */
class ReaderHandoverTest {

    @Test
    fun a_reader_that_survived_closing_refuses_the_next_open() {
        val handover = ReaderHandover()
        handover.recordClose(stopped = false) { true }
        assertTrue(handover.refuseWhileReading())
    }

    @Test
    fun a_reader_that_stopped_leaves_nothing_to_refuse() {
        val handover = ReaderHandover()
        handover.recordClose(stopped = true) { true }
        assertFalse("a stopped reader must not block the next connection", handover.refuseWhileReading())
    }

    @Test
    fun a_session_that_is_not_an_accessory_never_arms_the_record() {
        // closeSession closes whatever the current session is and reports "stopped" when it is
        // not an accessory at all (the `?: true` the old assertions could not see), so that
        // path must not arm the refusal.
        val handover = ReaderHandover()
        handover.recordClose(stopped = true) { error("a stopped close must not be asked about") }
        assertFalse(handover.refuseWhileReading())
    }

    @Test
    fun the_refusal_is_durable_rather_than_one_iteration_deep() {
        val handover = ReaderHandover()
        handover.recordClose(stopped = false) { true }
        repeat(3) { attempt ->
            assertTrue("attempt ${attempt + 1} must still be refused", handover.refuseWhileReading())
        }
    }

    @Test
    fun the_refusal_asks_about_the_reader_now_instead_of_a_snapshot() {
        val handover = ReaderHandover()
        var stillAlive = true
        handover.recordClose(stopped = false) { stillAlive }
        assertTrue(handover.refuseWhileReading())
        // The reader eventually returns; a snapshot taken at record time would refuse for ever.
        stillAlive = false
        assertFalse(handover.refuseWhileReading())
    }

    @Test
    fun clearing_the_record_lets_the_next_connection_through() {
        val handover = ReaderHandover()
        handover.recordClose(stopped = false) { true }
        handover.clear()
        assertFalse(handover.refuseWhileReading())
    }
}
