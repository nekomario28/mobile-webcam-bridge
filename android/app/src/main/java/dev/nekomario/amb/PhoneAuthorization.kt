package dev.nekomario.amb

import java.security.SecureRandom
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

/** One phone-owned session slot across USB and LAN. No bearer/empty-list bypass.
 * Callbacks only enqueue frames or close I/O/post UI work; they must not join a
 * reader which can be waiting for this owner's lock. */
class PhoneAuthorization(
    private val repository: ClientRepository,
    private val nowSeconds: () -> Long = { System.currentTimeMillis() / 1000 },
    private val nowMillis: () -> Long = { System.nanoTime() / 1_000_000 },
    private val onStorageFailure: () -> Unit,
) : AutoCloseable {
    class RequestEnded : IllegalStateException("approval request is no longer live")

    private val lock = Any()
    private var active: Session? = null
    // Volatile: persistence runs outside the monitor, so the latching flags are
    // read without it (AUTHREV-002).
    @Volatile private var failed = false
    @Volatile private var closed = false
    private var unknownCooldownUntil = Long.MIN_VALUE
    // Revocation must take effect before the durable write returns: a stalled
    // Keystore/commit call must not keep the live session authorized (REV-001).
    private val revokedIds = mutableSetOf<List<Byte>>()
    private val timer = Executors.newSingleThreadScheduledExecutor { task ->
        Thread(task, "mwb-client-authorization").apply { isDaemon = true }
    }.also { it.scheduleAtFixedRate({ tick() }, 250, 250, TimeUnit.MILLISECONDS) }

    fun clients(): List<ClientAuthorization.Client> = synchronized(lock) { stored { repository.snapshot() } }

    // Durable writes never run under [lock]: a stalled Keystore/commit call
    // would otherwise block the expiry timer and session teardown (AUTHREV-002).
    // The repository serialises its own mutations. Revocation is different from
    // renewal: it must stop the live session immediately, so it records the ID
    // in memory first and only then persists (REV-001).
    fun revoke(id: ByteArray) {
        synchronized(lock) {
            require(id.size == ClientAuthorization.CLIENT_ID_BYTES)
            revokedIds.add(id.toList())
            active?.checkAuthorized()
        }
        stored { repository.revoke(id) }
        synchronized(lock) { active?.checkAuthorized() }
    }

    fun renew(id: ByteArray) {
        stored { repository.renew(id, nowSeconds()) }
        synchronized(lock) { active?.checkAuthorized() }
    }

    /** Safe both inside and outside [lock]; [lock] is reentrant. */
    private fun <T> stored(action: () -> T): T {
        check(!failed && !closed) { "authorization storage unavailable" }
        try { return action() } catch (e: Exception) {
            synchronized(lock) {
                failed = true
                active?.finish("Authorization storage failed")
            }
            onStorageFailure()
            throw e
        }
    }

    fun openSession(
        transport: Int,
        phoneBinding: ByteArray,
        send: (Byte, ByteArray) -> Unit,
        onChanged: () -> Unit,
        onClosed: (String) -> Unit,
    ): Session = synchronized(lock) {
        check(!failed && !closed) { "authorization storage unavailable" }
        check(active == null) { "another authorization session is active" }
        require(transport == Wire.AUTH_USB || transport == Wire.AUTH_LAN)
        // The LAN adapter derives this from its own TLS certificate, never
        // peer input. USB explicitly has no cryptographic phone attestation.
        Wire.encodeClientChallenge(Wire.ClientChallenge(Wire.CLIENT_AUTH_VERSION, transport, phoneBinding, ByteArray(32)))
        Session(transport, phoneBinding.copyOf(), send, onChanged, onClosed).also { active = it }
    }

    /** The timer, not a blocked transport reader/UI, owns idle expiry. */
    fun tick() = synchronized(lock) {
        if (!failed && !closed) active?.tickLocked()
    }

    override fun close() = synchronized(lock) {
        closed = true
        active?.finish("Phone authorization closed")
        timer.shutdownNow()
        Unit
    }

    inner class Session internal constructor(
        private val transport: Int,
        private val phoneBinding: ByteArray,
        private val send: (Byte, ByteArray) -> Unit,
        private val onChanged: () -> Unit,
        private val onClosed: (String) -> Unit,
    ) : AutoCloseable {
        private var phase = Phase.NEW
        private val nonce = ByteArray(Wire.NONCE_BYTES).also { SecureRandom().nextBytes(it) }
        private var deadline = nowMillis() + PROOF_TIMEOUT_MS
        private var candidate: ClientAuthorization.Client? = null

        fun start() = synchronized(lock) {
            check(active === this && phase == Phase.NEW)
            phase = Phase.PROOF
            deadline = nowMillis() + PROOF_TIMEOUT_MS
            send(Wire.TYPE_CLIENT_CHALLENGE,
                Wire.encodeClientChallenge(Wire.ClientChallenge(Wire.CLIENT_AUTH_VERSION, transport, phoneBinding, nonce)))
            onChanged()
        }

        /** This immutable display record is a candidate, never a stored grant. */
        fun pending(): ClientAuthorization.Client? = synchronized(lock) {
            tickLocked()
            candidate?.takeIf { phase == Phase.PENDING }
        }

        fun isAuthorized(): Boolean = synchronized(lock) { checkAuthorized() }

        /**
         * The self-declared name of the PC this session is streaming to, or null.
         *
         * The phone shows it so "who am I sending video to?" has an answer, but the
         * name is the PC's own claim — the verified identity is the client id — so
         * callers are expected to say so rather than present it as verified.
         */
        fun authorizedLabel(): String? = synchronized(lock) {
            if (!checkAuthorized()) return@synchronized null
            candidate?.label
        }

        internal fun checkAuthorized(): Boolean {
            if (phase != Phase.AUTHORIZED || active !== this || failed || closed) return false
            val key = checkNotNull(candidate)
            if (revokedIds.contains(key.clientId.toList())) {
                finish("PC authorization revoked or expired")
                return false
            }
            val record = try {
                stored { repository.snapshot() }.firstOrNull { it.clientId.contentEquals(key.clientId) }
            } catch (_: Exception) { return false }
            val now = nowSeconds()
            if (record == null || !record.publicKey.contentEquals(key.publicKey) ||
                now < record.approvedAtUnixSeconds || now >= record.expiresAtUnixSeconds) {
                finish("PC authorization revoked or expired")
                return false
            }
            return true
        }

        /**
         * A host said hello (see `Wire.TYPE_HOST_HELLO`).
         *
         * - **Waiting for a proof**: re-send the same challenge. The peer may have read a
         *   challenge this session already sent and lost it (a previous host consumed the
         *   bytes), and re-sending the same nonce keeps this session the one that answers.
         * - **Already answered** (authorised, pending or denied): end the session. USB gives
         *   no disconnect signal, so this is the only way a reconnecting host gets a fresh
         *   challenge instead of a dead session.
         * - **Not yet challenged**: ignore; a challenge is on its way.
         */
        fun onHostHello() = synchronized(lock) {
            tickLocked()
            if (active !== this || phase == Phase.NEW || failed || closed) return@synchronized
            if (phase == Phase.PROOF) {
                send(Wire.TYPE_CLIENT_CHALLENGE,
                    Wire.encodeClientChallenge(Wire.ClientChallenge(Wire.CLIENT_AUTH_VERSION, transport, phoneBinding, nonce)))
                deadline = nowMillis() + PROOF_TIMEOUT_MS
                return@synchronized
            }
            finish("a new host asked for a fresh exchange")
        }

        /** Exactly one proof consumes this live session's challenge. Never
         * accepts an arbitrary caller-provided transcript or a replay. */
        fun receive(header: Wire.Header, payload: ByteArray) = synchronized(lock) {
            tickLocked()
            try {
                check(active === this && phase == Phase.PROOF) { "unexpected authorization proof" }
                require(header.type == Wire.TYPE_CLIENT_AUTH) { "PC authorization required before controls" }
                val proof = Wire.decodeClientAuth(payload)
                require(ClientAuthorization.validLabel(proof.label)) { "unsafe PC label" }
                val id = ClientAuthorization.deriveClientId(proof.publicKey)
                val message = Wire.clientAuthMessage(transport, phoneBinding, nonce, proof.publicKey, proof.label)
                val decision = ClientAuthorization.evaluate(id, proof.publicKey, message, proof.signature,
                    stored { repository.snapshot() }, nowSeconds())
                candidate = ClientAuthorization.Client(id, proof.publicKey, proof.label, 0, 0)
                when (decision.decision) {
                    ClientAuthorization.Decision.AUTHORIZED -> grant(decision.expiresAtUnixSeconds)
                    ClientAuthorization.Decision.REJECTED -> deny(decision.reason.code, decision.expiresAtUnixSeconds)
                    ClientAuthorization.Decision.PENDING -> {
                        if (nowMillis() < unknownCooldownUntil) deny(Wire.AUTH_REASON_UNKNOWN, 0)
                        else {
                            phase = Phase.PENDING
                            deadline = nowMillis() + APPROVAL_TIMEOUT_MS
                            send(Wire.TYPE_CLIENT_AUTH_RESULT,
                                Wire.encodeClientAuthResult(Wire.ClientAuthResult(Wire.AUTH_PENDING, Wire.AUTH_REASON_NONE, 0)))
                            onChanged()
                        }
                    }
                }
            } catch (e: Exception) {
                finish("PC authorization proof rejected")
                throw e
            }
        }

        private fun requireLiveApproval() {
            tickLocked()
            if (active !== this || phase != Phase.PENDING) throw RequestEnded()
        }

        fun approve() {
            val proposed = synchronized(lock) {
                requireLiveApproval()
                checkNotNull(candidate)
            }
            // Publish durably before granting, but never while holding the
            // monitor: a stalled save must not delay expiry or teardown.
            stored { repository.approve(proposed.clientId, proposed.publicKey, proposed.label, nowSeconds()) }
            synchronized(lock) {
                requireLiveApproval()
                val saved = stored { repository.snapshot() }.single { it.clientId.contentEquals(proposed.clientId) }
                // A fresh approval supersedes an earlier in-memory revocation of
                // the same PC, which only ever applied to the live session.
                revokedIds.remove(proposed.clientId.toList())
                grant(saved.expiresAtUnixSeconds)
            }
        }

        fun reject() = synchronized(lock) {
            requireLiveApproval()
            unknownCooldownUntil = nowMillis() + UNKNOWN_COOLDOWN_MS
            deny(Wire.AUTH_REASON_UNKNOWN, 0)
        }

        private fun grant(expiry: Long) {
            phase = Phase.AUTHORIZED
            send(Wire.TYPE_CLIENT_AUTH_RESULT,
                Wire.encodeClientAuthResult(Wire.ClientAuthResult(Wire.AUTH_AUTHORIZED, Wire.AUTH_REASON_NONE, expiry)))
            onChanged()
        }

        private fun deny(reason: Int, expiry: Long) {
            phase = Phase.DENIED
            // Keep one bounded grace interval to let the existing writer deliver
            // the rejection. No controls/capture are accepted in this phase.
            deadline = nowMillis() + REJECTION_FLUSH_MS
            send(Wire.TYPE_CLIENT_AUTH_RESULT,
                Wire.encodeClientAuthResult(Wire.ClientAuthResult(Wire.AUTH_REJECTED, reason, expiry)))
            onChanged()
        }

        internal fun tickLocked() {
            if (active !== this || phase == Phase.CLOSED) return
            if (phase == Phase.AUTHORIZED) { checkAuthorized(); return }
            // Absolute and non-renewable on purpose: a peer must not be able to keep its own
            // window open, and `stalled_proofs_and_pending_approvals_have_absolute_nonrenewable_deadlines`
            // pins that. A session that expires after an unanswered challenge is therefore
            // *by design* — which is why a reconnecting host needs `HOST_HELLO` rather than a
            // longer or renewable deadline here.
            if (nowMillis() >= deadline) finish("PC authorization deadline expired")
        }

        internal fun finish(reason: String) {
            if (phase == Phase.CLOSED) return
            // Disconnecting/abandoning a prompt cannot reset its rate limit.
            if (phase == Phase.PENDING) unknownCooldownUntil = nowMillis() + UNKNOWN_COOLDOWN_MS
            phase = Phase.CLOSED
            if (active === this) active = null
            onChanged()
            onClosed(reason)
        }

        override fun close() = synchronized(lock) { finish("PC disconnected") }
    }

    private enum class Phase { NEW, PROOF, PENDING, AUTHORIZED, DENIED, CLOSED }
    companion object {
        const val PROOF_TIMEOUT_MS = 5_000L
        const val APPROVAL_TIMEOUT_MS = 60_000L
        const val UNKNOWN_COOLDOWN_MS = 30_000L
        const val REJECTION_FLUSH_MS = 1_000L
    }
}
