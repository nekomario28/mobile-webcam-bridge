package dev.nekomario.amb

import org.junit.Assert.*
import org.junit.Test
import java.util.concurrent.CopyOnWriteArrayList
import dev.nekomario.amb.ClientCryptoFixture.keyPair
import dev.nekomario.amb.ClientCryptoFixture.sign

class PhoneAuthorizationTest {
    private val now = 1_800_000_000L
    private fun record(key: ByteArray, expiry: Long = now + ClientAuthorization.EXPIRY_SECONDS) =
        ClientAuthorization.Client(ClientAuthorization.deriveClientId(key), key, "studio-pc", now, expiry)

    private inner class Harness(initial: List<ClientAuthorization.Client> = emptyList()) : AutoCloseable {
        @Volatile var seconds = now
        @Volatile var millis = 0L
        var disk = ClientRecordCodec.encode(initial)
        var failWrite = false
        var storageFailures = 0
        var writes = 0
        val sent = CopyOnWriteArrayList<Pair<Byte, ByteArray>>()
        val closed = CopyOnWriteArrayList<String>()
        val repository = ClientRepository({ disk }, {
            if (failWrite) throw java.io.IOException("storage unavailable")
            disk = it.copyOf(); writes++
        })
        val owner = PhoneAuthorization(repository, { seconds }, { millis }) { storageFailures++ }
        lateinit var session: PhoneAuthorization.Session
        lateinit var challenge: Wire.ClientChallenge
        fun open(transport: Int = Wire.AUTH_LAN) {
            session = owner.openSession(transport, if (transport == Wire.AUTH_LAN) ClientCryptoFixture.phoneBinding() else ByteArray(32), { type, bytes -> sent += type to bytes }, {}, { closed += it })
            session.start()
            challenge = Wire.decodeClientChallenge(sent.last().second)
        }
        fun receive(payload: ByteArray) = session.receive(
            Wire.Header(Wire.TYPE_CLIENT_AUTH, 0, 1, payload.size, 0), payload)
        fun proof(key: Pair<ByteArray, java.security.PrivateKey>, label: String = "studio-pc",
            signedNonce: ByteArray = challenge.nonce, signedTransport: Int = challenge.transport,
            signedBinding: ByteArray = if (signedTransport == Wire.AUTH_USB) ByteArray(32) else challenge.phoneBinding,
            signedLabel: String = label, signingKey: java.security.PrivateKey = key.second): ByteArray {
            val message = Wire.clientAuthMessage(signedTransport, signedBinding, signedNonce, key.first, signedLabel)
            return Wire.encodeClientAuth(Wire.ClientAuth(Wire.CLIENT_AUTH_VERSION,
                key.first, label, sign(signingKey, message)))
        }
        fun results() = sent.filter { it.first == Wire.TYPE_CLIENT_AUTH_RESULT }.map { Wire.decodeClientAuthResult(it.second) }
        override fun close() { owner.close() }
    }

    @Test fun an_empty_list_requires_a_live_phone_approval_saved_before_authorization() {
        Harness().use { h ->
            h.open(); h.receive(h.proof(keyPair()))
            assertFalse(h.session.isAuthorized())
            assertNotNull(h.session.pending())
            assertEquals(listOf(Wire.AUTH_PENDING), h.results().map { it.status })
            assertTrue(h.repository.snapshot().isEmpty())
            h.session.approve()
            assertEquals(1, h.writes)
            assertEquals(1, ClientRecordCodec.decode(h.disk).size)
            assertTrue(h.session.isAuthorized())
            assertNull(h.session.pending())
            assertEquals(listOf(Wire.AUTH_PENDING, Wire.AUTH_AUTHORIZED), h.results().map { it.status })
        }
    }

    @Test fun a_known_pc_must_still_prove_possession_for_every_connection() {
        val key = keyPair()
        Harness(listOf(record(key.first))).use { h ->
            h.open()
            assertFalse(h.session.isAuthorized())
            h.receive(h.proof(key))
            assertTrue(h.session.isAuthorized())
            assertEquals(listOf(Wire.AUTH_AUTHORIZED), h.results().map { it.status })
            assertEquals(0, h.writes) // reconnect does not silently renew
        }
    }

    @Test fun expiry_revocation_and_clock_rollback_close_live_authorization() {
        val key = keyPair()
        for (transition in listOf("expire", "revoke", "clock-backward")) {
            Harness(listOf(record(key.first, now + 2))).use { h ->
                h.open(); h.receive(h.proof(key)); assertTrue(h.session.isAuthorized())
                when (transition) {
                    "expire" -> h.seconds += 2
                    "clock-backward" -> h.seconds -= 1
                    else -> h.owner.revoke(ClientAuthorization.deriveClientId(key.first))
                }
                h.owner.tick()
                assertFalse(transition, h.session.isAuthorized())
                assertEquals(1, h.closed.size)
                if (transition == "revoke") assertTrue(ClientRecordCodec.decode(h.disk).isEmpty())
            }
        }
    }

    @Test fun expired_known_clients_do_not_become_new_enrollment_requests() {
        val key = keyPair()
        Harness(listOf(record(key.first, now + 1))).use { h ->
            h.seconds += 1; h.open(); h.receive(h.proof(key))
            assertFalse(h.session.isAuthorized()); assertNull(h.session.pending())
            assertEquals(Wire.AUTH_REASON_EXPIRED, h.results().single().reason)
            assertThrows(IllegalStateException::class.java) { h.session.approve() }
            assertEquals(0, h.writes)
        }
    }

    @Test fun usb_also_requires_a_proof_and_explicit_durable_approval() {
        Harness().use { h ->
            val key = keyPair()
            h.open(Wire.AUTH_USB)
            assertArrayEquals(ByteArray(32), h.challenge.phoneBinding)
            assertFalse(h.session.isAuthorized())
            h.receive(h.proof(key))
            assertEquals(Wire.AUTH_PENDING, h.results().last().status)
            assertEquals(0, h.writes)
            h.session.approve()
            assertTrue(h.session.isAuthorized())
            assertEquals(Wire.AUTH_AUTHORIZED, h.results().last().status)
            assertNull(h.session.pending())
            assertArrayEquals(key.first, ClientRecordCodec.decode(h.disk).single().publicKey)
        }
    }

    @Test fun transport_nonce_label_and_signing_key_are_bound_to_the_proof() {
        val key = keyPair()
        for (field in listOf("transport", "phone", "nonce", "label", "key")) {
            Harness().use { h ->
                h.open()
                val payload = when (field) {
                    "transport" -> h.proof(key, signedTransport = Wire.AUTH_USB)
                    "phone" -> h.proof(key, signedBinding = ByteArray(32) { 0x7f })
                    "nonce" -> h.proof(key, signedNonce = h.challenge.nonce.copyOf().also { it[0] = (it[0].toInt() xor 1).toByte() })
                    "label" -> h.proof(key, signedLabel = "other-name")
                    else -> h.proof(key, signingKey = keyPair().second)
                }
                h.receive(payload)
                assertFalse(field, h.session.isAuthorized())
                assertNull(h.session.pending())
                assertEquals(Wire.AUTH_REASON_BAD_SIGNATURE, h.results().single().reason)
                assertEquals(0, h.writes)
            }
        }
    }

    @Test fun challenges_are_one_shot_and_a_new_session_rejects_the_old_proof() {
        Harness().use { h ->
            val key = keyPair()
            h.open(); val old = h.proof(key); val nonce = h.challenge.nonce.copyOf()
            h.receive(old)
            assertThrows(IllegalStateException::class.java) { h.receive(old) }
            assertThrows(IllegalStateException::class.java) { h.session.approve() }
            assertTrue(h.repository.snapshot().isEmpty())
            h.open()
            assertFalse(nonce.contentEquals(h.challenge.nonce))
            h.receive(old)
            assertEquals(Wire.AUTH_REASON_BAD_SIGNATURE, h.results().last().reason)
            assertFalse(h.session.isAuthorized())
        }
    }

    @Test fun disconnected_requests_cannot_be_approved_late_or_move_to_a_replacement_session() {
        Harness().use { h ->
            h.open(); h.receive(h.proof(keyPair()))
            val stale = h.session
            stale.close()
            h.millis += PhoneAuthorization.UNKNOWN_COOLDOWN_MS
            h.open(); h.receive(h.proof(keyPair()))
            assertThrows(PhoneAuthorization.RequestEnded::class.java) { stale.approve() }
            assertEquals(0, h.writes)
            assertFalse(h.session.isAuthorized())
            assertNotNull(h.session.pending())
            h.session.approve()
            assertTrue(h.session.isAuthorized())
        }
    }

    @Test fun abandoning_a_pending_request_cannot_bypass_the_prompt_cooldown() {
        Harness().use { h ->
            h.open(); h.receive(h.proof(keyPair())); h.session.close()
            h.open(); h.receive(h.proof(keyPair()))
            assertEquals(Wire.AUTH_REJECTED, h.results().last().status)
            assertNull(h.session.pending())
            h.session.close()
            h.millis += PhoneAuthorization.UNKNOWN_COOLDOWN_MS
            h.open(); h.receive(h.proof(keyPair()))
            assertEquals(Wire.AUTH_PENDING, h.results().last().status)
        }
    }

    @Test fun camera_controls_cannot_be_used_as_an_authorization_bypass() {
        for (type in listOf(Wire.TYPE_PING, Wire.TYPE_VIDEO_IDR_REQUEST, Wire.TYPE_PREPARE_QUALITY, Wire.TYPE_COMMIT_QUALITY)) {
            Harness().use { h ->
                h.open()
                assertThrows(IllegalArgumentException::class.java) {
                    h.session.receive(Wire.Header(type, 0, 1, 0, 0), byteArrayOf())
                }
                assertFalse(h.session.isAuthorized()); assertEquals(1, h.closed.size)
                assertEquals(0, h.writes)
            }
        }
    }

    @Test fun stalled_proofs_and_pending_approvals_have_absolute_nonrenewable_deadlines() {
        for (pending in listOf(false, true)) {
            Harness().use { h ->
                h.open()
                if (pending) h.receive(h.proof(keyPair()))
                h.millis += if (pending) PhoneAuthorization.APPROVAL_TIMEOUT_MS else PhoneAuthorization.PROOF_TIMEOUT_MS
                h.owner.tick()
                assertEquals(1, h.closed.size)
                assertFalse(h.session.isAuthorized())
                assertThrows(IllegalStateException::class.java) { h.session.approve() }
                assertEquals(0, h.writes)
            }
        }
    }

    @Test fun rejection_is_delivered_before_a_bounded_close_and_unknown_requests_cool_down() {
        Harness().use { h ->
            h.open(); h.receive(h.proof(keyPair())); h.session.reject()
            assertEquals(Wire.AUTH_REJECTED, h.results().last().status)
            assertFalse(h.session.isAuthorized()); assertTrue(h.closed.isEmpty())
            h.millis += PhoneAuthorization.REJECTION_FLUSH_MS; h.owner.tick()
            assertEquals(1, h.closed.size)
            h.open(); h.receive(h.proof(keyPair()))
            assertNull(h.session.pending()); assertEquals(Wire.AUTH_REJECTED, h.results().last().status)
            h.session.close()
            h.millis += PhoneAuthorization.UNKNOWN_COOLDOWN_MS
            h.open(); h.receive(h.proof(keyPair()))
            assertNotNull(h.session.pending())
        }
    }

    @Test fun a_failed_approval_or_revoke_closes_access_without_publishing_success() {
        val key = keyPair()
        for (revoke in listOf(false, true)) {
            Harness(if (revoke) listOf(record(key.first)) else emptyList()).use { h ->
                h.open(); h.receive(h.proof(key))
                val before = h.results().size
                h.failWrite = true
                assertThrows(IllegalStateException::class.java) {
                    if (revoke) h.owner.revoke(ClientAuthorization.deriveClientId(key.first)) else h.session.approve()
                }
                assertFalse(h.session.isAuthorized()); assertEquals(1, h.closed.size)
                assertEquals(1, h.storageFailures)
                assertEquals(before, h.results().size)
                assertThrows(IllegalStateException::class.java) { h.owner.clients() }
                assertEquals(if (revoke) 1 else 0, ClientRecordCodec.decode(h.disk).size)
            }
        }
    }

    @Test fun the_phone_has_one_cross_transport_session_slot() {
        Harness().use { h ->
            h.open(Wire.AUTH_USB)
            assertThrows(IllegalStateException::class.java) { h.open(Wire.AUTH_LAN) }
            h.session.close()
            h.open(Wire.AUTH_LAN)
            assertEquals(Wire.AUTH_LAN, h.challenge.transport)
        }
    }

    @Test fun the_real_timer_closes_an_idle_session_without_a_reader_or_ui_tick() {
        val closed = java.util.concurrent.CountDownLatch(1)
        val repository = ClientRepository({ null }, {})
        PhoneAuthorization(repository, onStorageFailure = { fail("no storage failure expected") }).use { owner ->
            val session = owner.openSession(Wire.AUTH_USB, ByteArray(32), { _, _ -> }, {}, { closed.countDown() })
            session.start()
            assertTrue("independent proof deadline did not close idle I/O", closed.await(7, java.util.concurrent.TimeUnit.SECONDS))
            assertFalse(session.isAuthorized())
            assertTrue(repository.snapshot().isEmpty())
        }
    }
}
