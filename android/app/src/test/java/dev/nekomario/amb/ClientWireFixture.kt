package dev.nekomario.amb

import java.net.Socket
import org.junit.Assert.*

/** Deliberate phone approval in socket fixtures, not a substitute for UI tests. */
object ClientWireFixture {
    fun session(socket: Socket, capabilities: (() -> Wire.Capabilities?)? = null,
        onQuality: (MediaSession, Wire.HostCommand) -> Unit): LanSession {
        val owner = PhoneAuthorization(ClientRepository({ null }, {}), onStorageFailure = { error("fixture storage failed") })
        return LanSession(socket, { _, _ -> owner.close() }, owner, ClientCryptoFixture.phoneBinding(),
            onAuthorization = { it.clientAuthorization?.takeIf { gate -> gate.pending() != null }?.approve() },
            capabilities = capabilities, onQuality = onQuality)
    }

    fun authorize(socket: Socket) {
        val (header, bytes) = Wire.readFrame(socket.getInputStream())
        assertEquals(Wire.TYPE_CLIENT_CHALLENGE, header.type)
        val challenge = Wire.decodeClientChallenge(bytes)
        assertEquals(Wire.AUTH_LAN, challenge.transport)
        assertArrayEquals(ClientCryptoFixture.phoneBinding(), challenge.phoneBinding)
        val (publicKey, privateKey) = ClientCryptoFixture.keyPair()
        val label = "fixture-pc"
        val message = Wire.clientAuthMessage(Wire.AUTH_LAN, ClientCryptoFixture.phoneBinding(), challenge.nonce, publicKey, label)
        val proof = Wire.encodeClientAuth(Wire.ClientAuth(Wire.CLIENT_AUTH_VERSION,
            publicKey, label, ClientCryptoFixture.sign(privateKey, message)))
        Wire.writeFrame(socket.getOutputStream(), Wire.Header(Wire.TYPE_CLIENT_AUTH, 0, 1, proof.size, 0), proof)
        fun result(): Wire.ClientAuthResult {
            val frame = Wire.readFrame(socket.getInputStream())
            assertEquals("no media/capabilities before approval", Wire.TYPE_CLIENT_AUTH_RESULT, frame.first.type)
            return Wire.decodeClientAuthResult(frame.second)
        }
        assertEquals(Wire.AUTH_PENDING, result().status)
        assertEquals(Wire.AUTH_AUTHORIZED, result().status)
    }
}
