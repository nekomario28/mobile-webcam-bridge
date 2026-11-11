package dev.nekomario.amb

import java.net.InetAddress
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class HandshakeAdmissionTest {
    private val a = InetAddress.getByName("192.168.1.10")
    private val b = InetAddress.getByName("192.168.1.11")
    private val c = InetAddress.getByName("192.168.1.12")

    @Test
    fun onePeerCannotTakeEverySlot() {
        val admission = HandshakeAdmission(maxTotal = 6, maxPerPeer = 2)
        assertTrue(admission.tryAdmit(a))
        assertTrue(admission.tryAdmit(a))
        assertFalse("third from the same address", admission.tryAdmit(a))
        assertTrue("another address still gets in", admission.tryAdmit(b))
        assertEquals(3, admission.inFlight())
        assertEquals(2, admission.inFlight(a))
    }

    @Test
    fun theTotalIsBoundedAcrossPeers() {
        val admission = HandshakeAdmission(maxTotal = 3, maxPerPeer = 2)
        assertTrue(admission.tryAdmit(a))
        assertTrue(admission.tryAdmit(a))
        assertTrue(admission.tryAdmit(b))
        assertFalse(admission.tryAdmit(c))
        admission.release(a)
        assertTrue("a released slot is reusable", admission.tryAdmit(c))
        assertFalse(admission.tryAdmit(b))
    }

    @Test
    fun releasingRestoresPerPeerCapacityAndIgnoresUnknownPeers() {
        val admission = HandshakeAdmission(maxTotal = 4, maxPerPeer = 1)
        assertTrue(admission.tryAdmit(a))
        assertFalse(admission.tryAdmit(a))
        admission.release(c) // never admitted: no effect
        assertEquals(1, admission.inFlight())
        admission.release(a)
        admission.release(a) // double release: no effect
        assertEquals(0, admission.inFlight())
        assertTrue(admission.tryAdmit(a))
    }

    @Test
    fun aNullPeerIsLimitedLikeAnyOther() {
        val admission = HandshakeAdmission(maxTotal = 4, maxPerPeer = 1)
        assertTrue(admission.tryAdmit(null))
        assertFalse(admission.tryAdmit(null))
        admission.release(null)
        assertEquals(0, admission.inFlight())
    }

    @Test
    fun concurrentAdmissionNeverExceedsTheBounds() {
        val admission = HandshakeAdmission(maxTotal = 6, maxPerPeer = 2)
        val peers = (1..8).map { InetAddress.getByName("10.0.0.$it") }
        val pool = Executors.newFixedThreadPool(16)
        val start = CountDownLatch(1)
        val maxSeen = AtomicInteger()
        val violations = AtomicInteger()
        repeat(2_000) { i ->
            pool.execute {
                start.await()
                val peer = peers[i % peers.size]
                if (admission.tryAdmit(peer)) {
                    val total = admission.inFlight()
                    maxSeen.accumulateAndGet(total) { x, y -> maxOf(x, y) }
                    if (total > 6 || admission.inFlight(peer) > 2) violations.incrementAndGet()
                    admission.release(peer)
                }
            }
        }
        start.countDown()
        pool.shutdown()
        assertTrue(pool.awaitTermination(30, TimeUnit.SECONDS))
        assertEquals(0, violations.get())
        assertTrue(maxSeen.get() <= 6)
        assertEquals(0, admission.inFlight())
    }
}
