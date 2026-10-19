package dev.nekomario.amb

import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.DataInputStream
import java.io.DataOutputStream
import java.nio.ByteBuffer
import java.nio.charset.CodingErrorAction

/** Bounded storage format, not a network frame. Identity is always derived from
 * the stored key; no redundant client ID can disagree with it. */
object ClientRecordCodec {
    const val MAX_BYTES = 2 + ClientAuthorization.MAX_CLIENTS * (65 + 1 + 32 + 8 + 8)

    private fun validate(clients: List<ClientAuthorization.Client>) {
        require(clients.size <= ClientAuthorization.MAX_CLIENTS) { "too many authorized PCs" }
        val ids = mutableSetOf<List<Byte>>()
        for (client in clients) {
            require(ClientAuthorization.validPublicKey(client.publicKey)) { "invalid stored public key" }
            require(client.clientId.contentEquals(ClientAuthorization.deriveClientId(client.publicKey))) { "stored client ID/key mismatch" }
            require(ids.add(client.clientId.toList())) { "duplicate authorized PC" }
            require(ClientAuthorization.validLabel(client.label)) { "invalid stored PC label" }
            require(client.approvedAtUnixSeconds >= 0 && client.expiresAtUnixSeconds <= 0xffffffffL &&
                client.expiresAtUnixSeconds > client.approvedAtUnixSeconds &&
                client.expiresAtUnixSeconds - client.approvedAtUnixSeconds <= ClientAuthorization.EXPIRY_SECONDS
            ) { "invalid stored authorization period" }
        }
    }

    fun encode(clients: List<ClientAuthorization.Client>): ByteArray {
        validate(clients)
        val bytes = ByteArrayOutputStream()
        DataOutputStream(bytes).use { out ->
            out.writeByte(1)
            out.writeByte(clients.size)
            for (client in clients) {
                val label = client.label.toByteArray(Charsets.UTF_8)
                out.write(client.publicKey)
                out.writeByte(label.size)
                out.write(label)
                out.writeLong(client.approvedAtUnixSeconds)
                out.writeLong(client.expiresAtUnixSeconds)
            }
        }
        return bytes.toByteArray()
    }

    fun decode(bytes: ByteArray): List<ClientAuthorization.Client> {
        require(bytes.size in 2..MAX_BYTES) { "invalid authorization storage size" }
        val input = DataInputStream(ByteArrayInputStream(bytes))
        require(input.readUnsignedByte() == 1) { "unsupported authorization storage version" }
        val count = input.readUnsignedByte()
        require(count <= ClientAuthorization.MAX_CLIENTS) { "too many authorized PCs" }
        val clients = List(count) {
            val key = ByteArray(65).also { input.readFully(it) }
            val length = input.readUnsignedByte()
            require(length <= ClientAuthorization.MAX_LABEL_BYTES) { "stored label exceeds byte limit" }
            val labelBytes = ByteArray(length).also { input.readFully(it) }
            val label = Charsets.UTF_8.newDecoder()
                .onMalformedInput(CodingErrorAction.REPORT).onUnmappableCharacter(CodingErrorAction.REPORT)
                .decode(ByteBuffer.wrap(labelBytes)).toString()
            ClientAuthorization.Client(ClientAuthorization.deriveClientId(key), key, label,
                input.readLong(), input.readLong())
        }
        require(input.read() == -1) { "trailing authorization storage bytes" }
        validate(clients)
        return clients
    }
}

/** One transactional owner for both transports. A failed write may have changed
 * the platform preferences' in-memory cache even if it did not reach disk, so
 * fail closed until the owner is explicitly recreated; never publish a grant. */
class ClientRepository(
    read: () -> ByteArray?,
    private val write: (ByteArray) -> Unit,
    /** Throws when the store has been deliberately reset since this owner was
     * created, which latches it shut; writable by default for the namespaces
     * that have no reset path. Not named `assert*`: those can be disabled, and
     * this guard is the only thing standing between a stale owner and a record
     * the user erased. */
    private val checkWritable: () -> Unit = {},
) {
    // Volatile immutable snapshots, published only after a successful write.
    // A reader must never queue behind a slow Keystore/commit call: the expiry
    // timer and the frame gate decide whether access continues, and delaying
    // them delays revoking access (AUTHREV-002).
    @Volatile private var clients = read()?.let(ClientRecordCodec::decode) ?: emptyList()
    @Volatile private var healthy = true

    /** Wait-free; the returned list is never mutated in place. */
    fun snapshot(): List<ClientAuthorization.Client> {
        check(healthy) { "authorization storage failed; reconnect after resolving storage" }
        return clients
    }

    @Synchronized fun approve(clientId: ByteArray, key: ByteArray, label: String, now: Long) {
        val next = ClientAuthorization.approve(clients, clientId, key, label, now)
            ?: error("PC approval is invalid or the authorization list is full")
        publish(next)
    }

    @Synchronized fun revoke(clientId: ByteArray) {
        require(clientId.size == ClientAuthorization.CLIENT_ID_BYTES)
        val before = clients
        val next = ClientAuthorization.revoke(before, clientId)
        if (next != before) publish(next)
    }

    @Synchronized fun renew(clientId: ByteArray, now: Long) {
        val before = clients
        require(before.any { it.clientId.contentEquals(clientId) }) { "PC is no longer registered" }
        publish(ClientAuthorization.renew(before, clientId, now))
    }

    private fun publish(next: List<ClientAuthorization.Client>) {
        val encoded = ClientRecordCodec.encode(next)
        try {
            // Before the write, so a writer from before a deliberate reset cannot
            // resurrect the record it erased; a refusal latches this owner shut.
            checkWritable()
            write(encoded)
        } catch (error: Exception) {
            healthy = false
            throw IllegalStateException("authorization could not be saved; access must stop", error)
        }
        clients = next.toList()
    }
}
