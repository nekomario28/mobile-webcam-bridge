package dev.nekomario.amb

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The phone's authorization list. No device and no PC: the list transitions and
 * the signature check are pure JVM logic, which is why they are tested here
 * rather than on an emulator.
 */
class ClientAuthorizationTest {
    private val now = 1_800_000_000L

    private fun keyPair() = ClientAuthorization.generateTestKeyPair()

    private fun signed(
        publicKey: ByteArray,
        privateKey: java.security.PrivateKey,
        label: String = "studio-pc",
        nonce: ByteArray = ByteArray(32) { 7 },
    ): Triple<ByteArray, ByteArray, ByteArray> {
        val clientId = ClientAuthorization.deriveClientId(publicKey)
        val message = ClientAuthorization.signedMessage(clientId, nonce, label)
        val der = ClientAuthorization.signForTest(privateKey, message)
        return Triple(clientId, message, derToRaw(der))
    }

    private fun client(
        clientId: ByteArray,
        publicKey: ByteArray,
        expires: Long = now + ClientAuthorization.EXPIRY_SECONDS,
        label: String = "studio-pc",
    ) = ClientAuthorization.Client(clientId, publicKey, label, now, expires)

    // --- the signature is the first gate -----------------------------------

    @Test
    fun a_valid_signature_authorizes_a_known_client() {
        val (publicKey, privateKey) = keyPair()
        val (clientId, message, signature) = signed(publicKey, privateKey)
        val clients = listOf(client(clientId, publicKey))
        val result = ClientAuthorization.evaluate(
            clientId, publicKey, message, signature, clients, now,
        )
        assertEquals(ClientAuthorization.Decision.AUTHORIZED, result.decision)
        assertEquals(now + ClientAuthorization.EXPIRY_SECONDS, result.expiresAtUnixSeconds)
    }

    @Test
    fun a_changed_message_is_rejected_before_the_list_is_consulted() {
        val (publicKey, privateKey) = keyPair()
        val (clientId, message, signature) = signed(publicKey, privateKey)
        val tampered = message.copyOf().also { it[0] = (it[0].toInt() xor 1).toByte() }
        val result = ClientAuthorization.evaluate(
            clientId, publicKey, tampered, signature, listOf(client(clientId, publicKey)), now,
        )
        assertEquals(ClientAuthorization.Decision.REJECTED, result.decision)
        assertEquals(ClientAuthorization.Reason.BAD_SIGNATURE, result.reason)
    }

    @Test
    fun a_relabelled_message_is_rejected() {
        val (publicKey, privateKey) = keyPair()
        val (clientId, message, signature) = signed(publicKey, privateKey)
        val relabelled = ClientAuthorization.signedMessage(
            clientId, ByteArray(32) { 7 }, "other-name",
        )
        val result = ClientAuthorization.evaluate(
            clientId, publicKey, relabelled, signature, listOf(client(clientId, publicKey)), now,
        )
        assertEquals(ClientAuthorization.Reason.BAD_SIGNATURE, result.reason)
    }

    @Test
    fun another_clients_key_cannot_answer_for_this_one() {
        val (publicKey, privateKey) = keyPair()
        val (theirs, theirKey) = keyPair()
        val (_, message, signature) = signed(theirs, theirKey)
        // Present the attacker's valid signature under the victim's client ID.
        val result = ClientAuthorization.evaluate(
            ClientAuthorization.deriveClientId(publicKey), publicKey, message, signature,
            listOf(client(ClientAuthorization.deriveClientId(publicKey), publicKey)), now,
        )
        assertEquals(ClientAuthorization.Reason.BAD_SIGNATURE, result.reason)
    }

    @Test
    fun a_lying_client_id_is_rejected_even_with_a_valid_signature() {
        val (publicKey, privateKey) = keyPair()
        val (_, message, signature) = signed(publicKey, privateKey)
        // A real signature over a message whose client ID is not the derived one.
        val claimed = ByteArray(16) { 0xab.toByte() }
        val result = ClientAuthorization.evaluate(
            claimed, publicKey, message, signature, emptyList(), now,
        )
        assertEquals(ClientAuthorization.Reason.BAD_SIGNATURE, result.reason)
    }

    @Test
    fun wrong_sized_fields_are_rejected_before_any_crypto() {
        val (publicKey, privateKey) = keyPair()
        val (clientId, message, signature) = signed(publicKey, privateKey)
        for (bad in listOf(
            Triple(clientId.copyOf(8), publicKey, signature),
            Triple(clientId, publicKey.copyOf(33), signature),
            Triple(clientId, publicKey, signature.copyOf(32)),
        )) {
            val result = ClientAuthorization.evaluate(
                bad.first, bad.second, message, bad.third, emptyList(), now,
            )
            assertEquals(ClientAuthorization.Reason.BAD_SIGNATURE, result.reason)
        }
    }

    // --- the list decides the rest -----------------------------------------

    @Test
    fun an_unknown_client_is_pending_not_rejected() {
        val (publicKey, privateKey) = keyPair()
        val (clientId, message, signature) = signed(publicKey, privateKey)
        val result = ClientAuthorization.evaluate(
            clientId, publicKey, message, signature, emptyList(), now,
        )
        assertEquals(ClientAuthorization.Decision.PENDING, result.decision)
        assertEquals(ClientAuthorization.Reason.NONE, result.reason)
    }

    @Test
    fun an_expired_client_is_rejected_with_its_expiry() {
        val (publicKey, privateKey) = keyPair()
        val (clientId, message, signature) = signed(publicKey, privateKey)
        val expired = now + 1
        val clients = listOf(client(clientId, publicKey, expires = expired))
        val result = ClientAuthorization.evaluate(
            clientId, publicKey, message, signature, clients, expired,
        )
        assertEquals(ClientAuthorization.Reason.EXPIRED, result.reason)
        assertEquals(expired, result.expiresAtUnixSeconds)
    }

    @Test
    fun a_full_list_rejects_an_unknown_client() {
        val (publicKey, privateKey) = keyPair()
        val (clientId, message, signature) = signed(publicKey, privateKey)
        val full = (0 until ClientAuthorization.MAX_CLIENTS).map { index ->
            val (otherKey, _) = keyPair()
            client(ClientAuthorization.deriveClientId(otherKey), otherKey, label = "pc-$index")
        }
        val result = ClientAuthorization.evaluate(
            clientId, publicKey, message, signature, full, now,
        )
        assertEquals(ClientAuthorization.Reason.TOO_MANY, result.reason)
    }

    @Test
    fun a_full_list_still_authorizes_its_own_clients() {
        val (publicKey, privateKey) = keyPair()
        val (clientId, message, signature) = signed(publicKey, privateKey)
        val full = (0 until ClientAuthorization.MAX_CLIENTS).map { index ->
            val (otherKey, _) = keyPair()
            client(ClientAuthorization.deriveClientId(otherKey), otherKey, label = "pc-$index")
        } + client(clientId, publicKey)
        val result = ClientAuthorization.evaluate(
            clientId, publicKey, message, signature, full, now,
        )
        assertEquals(ClientAuthorization.Decision.AUTHORIZED, result.decision)
    }

    // --- the transitions ---------------------------------------------------

    @Test
    fun approving_adds_one_entry_with_a_fresh_expiry() {
        val (publicKey, _) = keyPair()
        val clientId = ClientAuthorization.deriveClientId(publicKey)
        val approved = ClientAuthorization.approve(
            emptyList(), clientId, publicKey, "studio-pc", now,
        )!!
        assertEquals(1, approved.size)
        assertEquals(clientId.toList(), approved.single().clientId.toList())
        assertEquals(now + ClientAuthorization.EXPIRY_SECONDS, approved.single().expiresAtUnixSeconds)
    }

    @Test
    fun approving_again_refreshes_rather_than_stacking() {
        val (publicKey, _) = keyPair()
        val clientId = ClientAuthorization.deriveClientId(publicKey)
        val first = ClientAuthorization.approve(emptyList(), clientId, publicKey, "a", now)!!
        val later = now + 1000
        val second = ClientAuthorization.approve(first, clientId, publicKey, "a", later)!!
        assertEquals(1, second.size)
        assertEquals(later + ClientAuthorization.EXPIRY_SECONDS, second.single().expiresAtUnixSeconds)
    }

    @Test
    fun approving_beyond_the_cap_is_refused_not_silently_dropped() {
        val full = (0 until ClientAuthorization.MAX_CLIENTS).map {
            val (otherKey, _) = keyPair()
            client(ClientAuthorization.deriveClientId(otherKey), otherKey)
        }
        val (publicKey, _) = keyPair()
        assertNull(
            ClientAuthorization.approve(
                full, ClientAuthorization.deriveClientId(publicKey), publicKey, "one too many", now,
            ),
        )
    }

    @Test
    fun revoking_removes_exactly_one_client() {
        val (first, _) = keyPair()
        val (second, _) = keyPair()
        val firstId = ClientAuthorization.deriveClientId(first)
        val secondId = ClientAuthorization.deriveClientId(second)
        val clients = listOf(client(firstId, first), client(secondId, second))
        val after = ClientAuthorization.revoke(clients, firstId)
        assertEquals(1, after.size)
        assertEquals(secondId.toList(), after.single().clientId.toList())
    }

    @Test
    fun a_revoked_client_is_unknown_again() {
        val (publicKey, privateKey) = keyPair()
        val (clientId, message, signature) = signed(publicKey, privateKey)
        val before = listOf(client(clientId, publicKey))
        val after = ClientAuthorization.revoke(before, clientId)
        val result = ClientAuthorization.evaluate(
            clientId, publicKey, message, signature, after, now,
        )
        assertEquals(ClientAuthorization.Decision.PENDING, result.decision)
    }

    @Test
    fun renewing_moves_both_the_expiry_and_the_approval_time() {
        val (publicKey, _) = keyPair()
        val clientId = ClientAuthorization.deriveClientId(publicKey)
        val clients = listOf(client(clientId, publicKey, expires = now + 10))
        val later = now + 500
        val renewed = ClientAuthorization.renew(clients, clientId, later)
        // A renewal is a fresh approval, so both timestamps move; the entry is
        // still one entry, not a second one.
        assertEquals(1, renewed.size)
        assertEquals(later, renewed.single().approvedAtUnixSeconds)
        assertEquals(later + ClientAuthorization.EXPIRY_SECONDS, renewed.single().expiresAtUnixSeconds)
    }

    // --- the canonical cross-language signature ----------------------------

    @Test
    fun the_rust_generated_signature_verifies_here() {
        val fixture = locateFixture("protocol/fixtures/client-signature-v1.bin").readBytes()
        val publicKey = fixture.copyOfRange(0, 65)
        val message = fixture.copyOfRange(65, 65 + 57)
        val signature = fixture.copyOfRange(65 + 57, 65 + 57 + 64)
        assertTrue(ClientAuthorization.verify(publicKey, message, signature))
        // The client ID in the message is the derived one.
        assertEquals(
            ClientAuthorization.deriveClientId(publicKey).toList(),
            message.copyOfRange(0, 16).toList(),
        )
        // One changed byte anywhere in the signed message fails.
        for (index in listOf(0, 16, 48, 56)) {
            val tampered = message.copyOf().also { it[index] = (it[index].toInt() xor 1).toByte() }
            assertFalse(ClientAuthorization.verify(publicKey, tampered, signature))
        }
    }

    @Test
    fun a_platform_generated_signature_round_trips() {
        val (publicKey, privateKey) = keyPair()
        assertEquals(65, publicKey.size)
        assertEquals(0x04.toByte(), publicKey[0])
        val (clientId, message, signature) = signed(publicKey, privateKey)
        assertEquals(64, signature.size)
        assertTrue(ClientAuthorization.verify(publicKey, message, signature))
        assertEquals(clientId.toList(), ClientAuthorization.deriveClientId(publicKey).toList())
    }

    /** The DER wrapper is where a fixed-width signature could silently break:
     * a stripped or unpadded integer verifies against nothing. */
    @Test
    fun the_der_wrapper_preserves_the_significant_bytes() {
        // A raw signature whose halves start with a zero byte and with a high
        // bit set covers both padding directions.
        val raw = ByteArray(64)
        raw[0] = 0          // r's leading zero is significant
        raw[32] = 0x80.toByte()  // s needs a sign byte in DER
        val der = ClientAuthorization.rawSignatureToDer(raw)
        // r starts with 0x00, so its high bit is clear and DER needs no sign
        // byte; s starts with 0x80, so DER adds one. 34 + 35 = 69 contents.
        assertEquals(0x30.toByte(), der[0])
        assertEquals(69, der[1].toInt() and 0xff)
        assertEquals(71, der.size)
        assertEquals(0x02.toByte(), der[2])
        assertEquals(32, der[3].toInt() and 0xff)
        assertEquals(0, der[4].toInt())            // r keeps its significant zero
        assertEquals(0x02.toByte(), der[36])
        assertEquals(33, der[37].toInt() and 0xff)
        assertEquals(0, der[38].toInt())           // s gained a sign byte
        assertEquals(0x80.toByte(), der[39])
    }

    @Test
    fun the_result_maps_onto_the_wire_reasons() {
        assertEquals(Wire.AUTH_AUTHORIZED, ClientAuthorization.Result(
            ClientAuthorization.Decision.AUTHORIZED, ClientAuthorization.Reason.NONE, now,
        ).toWire().status)
        assertEquals(Wire.AUTH_PENDING, ClientAuthorization.Result(
            ClientAuthorization.Decision.PENDING, ClientAuthorization.Reason.NONE, 0,
        ).toWire().status)
        assertEquals(Wire.AUTH_REASON_EXPIRED, ClientAuthorization.Result(
            ClientAuthorization.Decision.REJECTED, ClientAuthorization.Reason.EXPIRED, now,
        ).toWire().reason)
    }

    /** The platform signs DER; the wire carries raw r||s. This converts back so
     * a test can produce a signature the way a Rust PC does. */
    private fun derToRaw(der: ByteArray): ByteArray {
        var offset = 0
        fun integer(): ByteArray {
            assertEquals(0x02.toByte(), der[offset]); offset += 1
            val length = der[offset].toInt() and 0xff; offset += 1
            val body = der.copyOfRange(offset, offset + length); offset += length
            return body
        }
        assertEquals(0x30.toByte(), der[offset]); offset += 1
        der[offset].toInt(); offset += 1
        val r = integer()
        val s = integer()
        assertEquals(der.size, offset)
        fun fixed32(value: ByteArray): ByteArray {
            // A DER integer may carry a sign byte; that one is not part of the
            // coordinate. Any other leading zero is significant.
            val body = if (value.size == 33 && value[0] == 0.toByte()) {
                value.copyOfRange(1, 33)
            } else {
                value
            }
            require(body.size <= 32) { "integer wider than 32 bytes" }
            val out = ByteArray(32)
            body.copyInto(out, 32 - body.size)
            return out
        }
        return fixed32(r) + fixed32(s)
    }

    private fun locateFixture(relative: String): java.io.File {
        var dir: java.io.File? = java.io.File("").absoluteFile
        repeat(5) {
            val candidate = java.io.File(dir, relative)
            if (candidate.isFile) return candidate
            dir = dir?.parentFile
        }
        throw AssertionError("fixture not found: $relative")
    }
}
