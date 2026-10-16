package dev.nekomario.amb

import java.io.InputStream
import java.io.OutputStream
import java.nio.ByteBuffer
import java.nio.ByteOrder

object Wire {
    const val MAGIC = 0x31424d41
    const val VERSION: Byte = 1
    const val HEADER_SIZE = 24
    const val MAX_PAYLOAD = 16 * 1024 * 1024

    const val TYPE_HELLO: Byte = 0x01
    const val TYPE_HELLO_ACK: Byte = 0x02
    const val TYPE_PING: Byte = 0x03
    const val TYPE_PONG: Byte = 0x04
    // Quality control extension (slice 02). Types stay in the control space;
    // both languages read protocol/fixtures/quality-capabilities-v1.bin.
    const val TYPE_QUALITY_CAPABILITIES: Byte = 0x05
    const val TYPE_PREPARE_QUALITY: Byte = 0x06
    const val TYPE_QUALITY_RESULT: Byte = 0x07
    const val TYPE_PAUSED: Byte = 0x08
    const val TYPE_COMMIT_QUALITY: Byte = 0x09
    /** Per-PC authorization (designed, not yet wired). The phone challenges,
     * the PC proves possession of its key, the phone answers. */
    const val TYPE_CLIENT_CHALLENGE: Byte = 0x0a
    const val TYPE_CLIENT_AUTH: Byte = 0x0b
    const val TYPE_CLIENT_AUTH_RESULT: Byte = 0x0c

    const val TYPE_VIDEO_CONFIG: Byte = 0x10
    const val TYPE_VIDEO_AU: Byte = 0x11
    const val TYPE_VIDEO_IDR_REQUEST: Byte = 0x12
    const val TYPE_STATS: Byte = 0x20
    const val TYPE_ERROR: Byte = 0x7f

    const val VIDEO_FLAG_KEYFRAME: Short = 0x0001
    const val VIDEO_FLAG_CONFIG_INCLUDED: Short = 0x0002
    const val VIDEO_FLAG_DISCONTINUITY: Short = 0x0004

    /** Control payload limit; the counted capability list must fit inside it. */
    const val MAX_CONTROL_PAYLOAD = 16 * 1024
    const val MAX_QUALITY_MODES = 256
    const val QUALITY_MODE_BYTES = 12

    /** Camera permission / feature state advertised with the capability list. */
    const val PERMISSION_READY = 0
    const val PERMISSION_PENDING = 1
    const val PERMISSION_UNAVAILABLE = 2

    /** PrepareQuality outcome states. */
    const val RESULT_STAGED = 3
    const val RESULT_SWITCHING = 0
    const val RESULT_REJECTED = 1
    const val RESULT_APPLIED = 2
    const val REASON_NONE = 0
    const val REASON_PERMISSION = 1
    const val REASON_FOREGROUND = 2
    const val REASON_UNSUPPORTED_MODE = 3
    const val REASON_STALE_REVISION = 4

    /** ClientAuthResult states and reasons. */
    const val AUTH_AUTHORIZED = 0
    const val AUTH_PENDING = 1
    const val AUTH_REJECTED = 2
    const val AUTH_REASON_NONE = 0
    const val AUTH_REASON_UNKNOWN = 1
    const val AUTH_REASON_REVOKED = 2
    const val AUTH_REASON_EXPIRED = 3
    const val AUTH_REASON_TOO_MANY = 4
    const val AUTH_REASON_BAD_SIGNATURE = 5

    /** A PC-proposed label. Attacker-chosen: never an identity. */
    const val MAX_CLIENT_LABEL = 32
    const val CLIENT_ID_BYTES = 16
    const val SIGNATURE_BYTES = 64
    const val NONCE_BYTES = 32

    data class QualityMode(val width: Int, val height: Int, val targetFps: Int, val bitrate: Int)

    data class Capabilities(
        val extensionVersion: Int,
        val permission: Int,
        val catalogRevision: Int,
        val streamEpoch: Long,
        val modes: List<QualityMode>,
    )

    data class PrepareQuality(
        val requestId: Long,
        val catalogRevision: Int,
        val width: Int,
        val height: Int,
        val targetFps: Int,
    )

    data class QualityResult(
        val requestId: Long,
        val status: Int,
        val reason: Int,
        val appliedWidth: Int,
        val appliedHeight: Int,
        val appliedFps: Int,
    )

    /** Unreleased hard cutover: v2 carries the enrollment public key. */
    const val CLIENT_AUTH_VERSION = 2
    const val CLIENT_PUBLIC_KEY_BYTES = 65
    const val PHONE_BINDING_BYTES = 32
    const val AUTH_USB = 0
    const val AUTH_LAN = 1
    const val CLIENT_AUTH_MIN_BYTES = 1 + CLIENT_PUBLIC_KEY_BYTES + 1 + SIGNATURE_BYTES
    const val CLIENT_AUTH_MAX_BYTES = CLIENT_AUTH_MIN_BYTES + MAX_CLIENT_LABEL

    data class ClientChallenge(val version: Int, val transport: Int, val phoneBinding: ByteArray, val nonce: ByteArray) {
        override fun equals(other: Any?) = other is ClientChallenge &&
            version == other.version && transport == other.transport && phoneBinding.contentEquals(other.phoneBinding) && nonce.contentEquals(other.nonce)
        override fun hashCode() = 31 * (31 * (31 * version + transport) + phoneBinding.contentHashCode()) + nonce.contentHashCode()
    }

    /** The ID is derived from the transferred point, not trusted peer input. */
    data class ClientAuth(
        val version: Int,
        val publicKey: ByteArray,
        val label: String,
        val signature: ByteArray,
    ) {
        override fun equals(other: Any?) = other is ClientAuth &&
            version == other.version && publicKey.contentEquals(other.publicKey) &&
            label == other.label && signature.contentEquals(other.signature)
        override fun hashCode() =
            31 * (31 * (31 * version + publicKey.contentHashCode()) + label.hashCode()) +
                signature.contentHashCode()
    }

    data class ClientAuthResult(val status: Int, val reason: Int, val expiresAtUnixSeconds: Long)

    fun sha256(bytes: ByteArray): ByteArray =
        java.security.MessageDigest.getInstance("SHA-256").digest(bytes)

    private fun requireTransport(transport: Int) {
        require(transport == AUTH_USB || transport == AUTH_LAN) { "unknown authorization transport" }
    }

    private fun requirePhoneBinding(transport: Int, binding: ByteArray) {
        require(binding.size == PHONE_BINDING_BYTES)
        require(if (transport == AUTH_USB) binding.all { it == 0.toByte() } else binding.any { it != 0.toByte() }) {
            "invalid phone certificate binding"
        }
    }

    private fun strictLabel(bytes: ByteArray): String {
        val label = Charsets.UTF_8.newDecoder()
            .onMalformedInput(java.nio.charset.CodingErrorAction.REPORT)
            .onUnmappableCharacter(java.nio.charset.CodingErrorAction.REPORT)
            .decode(ByteBuffer.wrap(bytes)).toString()
        require(bytes.size <= MAX_CLIENT_LABEL && label.isNotBlank() && label.none { it.isISOControl() }) {
            "invalid client label"
        }
        return label
    }

    /** Domain separation, then the phone-owned channel/nonce and exact point/label.
     * A random nonce belongs to one live session and is consumed once by its owner. */
    fun clientAuthMessage(transport: Int, phoneBinding: ByteArray, nonce: ByteArray, publicKey: ByteArray, label: String): ByteArray {
        requireTransport(transport)
        requirePhoneBinding(transport, phoneBinding)
        require(nonce.size == NONCE_BYTES)
        require(publicKey.size == CLIENT_PUBLIC_KEY_BYTES && publicKey[0] == 4.toByte())
        val text = label.toByteArray(Charsets.UTF_8)
        require(strictLabel(text) == label) { "client label is not lossless UTF-8" }
        return "AMB1 PC authorization\u0000v2\u0000".toByteArray(Charsets.US_ASCII) +
            byteArrayOf(transport.toByte()) + phoneBinding + nonce + publicKey + byteArrayOf(text.size.toByte()) + text
    }

    fun encodeClientChallenge(challenge: ClientChallenge): ByteArray {
        require(challenge.version == CLIENT_AUTH_VERSION) { "unsupported client challenge version" }
        requireTransport(challenge.transport)
        requirePhoneBinding(challenge.transport, challenge.phoneBinding)
        require(challenge.nonce.size == NONCE_BYTES)
        return byteArrayOf(challenge.version.toByte(), challenge.transport.toByte()) + challenge.phoneBinding + challenge.nonce
    }

    fun decodeClientChallenge(payload: ByteArray): ClientChallenge {
        require(payload.size == 2 + PHONE_BINDING_BYTES + NONCE_BYTES) { "client challenge payload must be 66 bytes" }
        require(payload[0].toInt() == CLIENT_AUTH_VERSION) { "unsupported client challenge version" }
        val transport = payload[1].toInt() and 0xff
        requireTransport(transport)
        val binding = payload.copyOfRange(2, 2 + PHONE_BINDING_BYTES)
        requirePhoneBinding(transport, binding)
        return ClientChallenge(CLIENT_AUTH_VERSION, transport, binding, payload.copyOfRange(2 + PHONE_BINDING_BYTES, payload.size))
    }

    fun encodeClientAuth(auth: ClientAuth): ByteArray {
        require(auth.version == CLIENT_AUTH_VERSION) { "unsupported client auth version" }
        require(auth.publicKey.size == CLIENT_PUBLIC_KEY_BYTES && auth.publicKey[0] == 4.toByte())
        require(auth.signature.size == SIGNATURE_BYTES)
        val label = auth.label.toByteArray(Charsets.UTF_8)
        require(strictLabel(label) == auth.label) { "client label is not lossless UTF-8" }
        return byteArrayOf(auth.version.toByte()) + auth.publicKey +
            byteArrayOf(label.size.toByte()) + label + auth.signature
    }

    fun decodeClientAuth(payload: ByteArray): ClientAuth {
        require(payload.size in CLIENT_AUTH_MIN_BYTES..CLIENT_AUTH_MAX_BYTES) { "invalid client auth length" }
        require(payload[0].toInt() == CLIENT_AUTH_VERSION) { "unsupported client auth version" }
        val offset = 1 + CLIENT_PUBLIC_KEY_BYTES
        val labelLength = payload[offset].toInt() and 0xff
        require(labelLength <= MAX_CLIENT_LABEL) { "client label exceeds $MAX_CLIENT_LABEL bytes" }
        require(payload.size == CLIENT_AUTH_MIN_BYTES + labelLength) { "client auth length does not match label" }
        val key = payload.copyOfRange(1, offset)
        require(key[0] == 4.toByte()) { "client key must be uncompressed P-256" }
        return ClientAuth(CLIENT_AUTH_VERSION, key,
            strictLabel(payload.copyOfRange(offset + 1, offset + 1 + labelLength)),
            payload.copyOfRange(payload.size - SIGNATURE_BYTES, payload.size))
    }

    private fun validateAuthResult(result: ClientAuthResult) {
        require(result.expiresAtUnixSeconds in 0..0xffff_ffffL)
        require(when (result.status) {
            AUTH_AUTHORIZED -> result.reason == AUTH_REASON_NONE && result.expiresAtUnixSeconds > 0
            AUTH_PENDING -> result.reason == AUTH_REASON_NONE && result.expiresAtUnixSeconds == 0L
            AUTH_REJECTED -> result.reason in AUTH_REASON_UNKNOWN..AUTH_REASON_BAD_SIGNATURE &&
                (result.reason == AUTH_REASON_EXPIRED || result.expiresAtUnixSeconds == 0L)
            else -> false
        }) { "invalid client authorization outcome" }
    }

    fun encodeClientAuthResult(result: ClientAuthResult): ByteArray {
        validateAuthResult(result)
        return ByteBuffer.allocate(6).order(ByteOrder.LITTLE_ENDIAN)
            .put(result.status.toByte()).put(result.reason.toByte())
            .putInt(result.expiresAtUnixSeconds.toInt()).array()
    }

    fun decodeClientAuthResult(payload: ByteArray): ClientAuthResult {
        require(payload.size == 6) { "client auth result payload must be 6 bytes" }
        val buffer = ByteBuffer.wrap(payload).order(ByteOrder.LITTLE_ENDIAN)
        return ClientAuthResult(buffer.get().toInt() and 0xff, buffer.get().toInt() and 0xff,
            buffer.int.toLong() and 0xffff_ffffL).also { validateAuthResult(it) }
    }

    data class Epoch(val requestId: Long, val streamEpoch: Long)

    sealed interface HostCommand {
        data class Prepare(val request: PrepareQuality) : HostCommand
        data class Commit(val epoch: Epoch) : HostCommand
    }

    /**
     * Fixed little-endian fields, then a counted mode list. The length is
     * validated against the count before the list is built.
     */
    fun encodeCapabilities(capabilities: Capabilities): ByteArray {
        require(capabilities.modes.size <= MAX_QUALITY_MODES) {
            "quality capability list exceeds $MAX_QUALITY_MODES modes"
        }
        val buffer = ByteBuffer.allocate(10 + capabilities.modes.size * QUALITY_MODE_BYTES)
            .order(ByteOrder.LITTLE_ENDIAN)
        buffer.put(capabilities.extensionVersion.toByte())
        buffer.put(capabilities.permission.toByte())
        buffer.putShort(capabilities.catalogRevision.toShort())
        buffer.putInt(capabilities.streamEpoch.toInt())
        buffer.putShort(capabilities.modes.size.toShort())
        for (mode in capabilities.modes) {
            buffer.putShort(mode.width.toShort())
            buffer.putShort(mode.height.toShort())
            buffer.putShort(mode.targetFps.toShort())
            buffer.putShort(0)
            buffer.putInt(mode.bitrate)
        }
        require(buffer.position() <= MAX_CONTROL_PAYLOAD) {
            "quality capabilities payload exceeds the control limit"
        }
        return buffer.array()
    }

    fun decodeCapabilities(payload: ByteArray): Capabilities {
        require(payload.size >= 10) { "quality capabilities payload is truncated" }
        val buffer = ByteBuffer.wrap(payload).order(ByteOrder.LITTLE_ENDIAN)
        val version = buffer.get().toInt() and 0xff
        val permission = buffer.get().toInt() and 0xff
        val revision = buffer.short.toInt() and 0xffff
        val epoch = buffer.int.toLong() and 0xffffffffL
        val count = buffer.short.toInt() and 0xffff
        require(count <= MAX_QUALITY_MODES) {
            "quality capability list exceeds $MAX_QUALITY_MODES modes"
        }
        require(payload.size == 10 + count * QUALITY_MODE_BYTES) {
            "quality capabilities length does not match the mode count"
        }
        require(payload.size <= MAX_CONTROL_PAYLOAD) {
            "quality capabilities payload exceeds the control limit"
        }
        val modes = ArrayList<QualityMode>(count)
        repeat(count) {
            val width = buffer.short.toInt() and 0xffff
            val height = buffer.short.toInt() and 0xffff
            val targetFps = buffer.short.toInt() and 0xffff
            buffer.short // reserved
            val bitrate = buffer.int
            modes.add(QualityMode(width, height, targetFps, bitrate))
        }
        return Capabilities(version, permission, revision, epoch, modes)
    }

    fun encodePrepareQuality(request: PrepareQuality): ByteArray {
        val buffer = ByteBuffer.allocate(14).order(ByteOrder.LITTLE_ENDIAN)
        buffer.putInt(request.requestId.toInt())
        buffer.putShort(request.catalogRevision.toShort())
        buffer.putShort(request.width.toShort())
        buffer.putShort(request.height.toShort())
        buffer.putShort(request.targetFps.toShort())
        buffer.putShort(0)
        return buffer.array()
    }

    fun decodePrepareQuality(payload: ByteArray): PrepareQuality {
        require(payload.size == 14) { "prepare quality payload must be 14 bytes" }
        val buffer = ByteBuffer.wrap(payload).order(ByteOrder.LITTLE_ENDIAN)
        val requestId = buffer.int.toLong() and 0xffffffffL
        val revision = buffer.short.toInt() and 0xffff
        val width = buffer.short.toInt() and 0xffff
        val height = buffer.short.toInt() and 0xffff
        val targetFps = buffer.short.toInt() and 0xffff
        return PrepareQuality(requestId, revision, width, height, targetFps)
    }

    fun encodeQualityResult(result: QualityResult): ByteArray {
        val buffer = ByteBuffer.allocate(14).order(ByteOrder.LITTLE_ENDIAN)
        buffer.putInt(result.requestId.toInt())
        buffer.put(result.status.toByte())
        buffer.put(result.reason.toByte())
        buffer.putShort(result.appliedWidth.toShort())
        buffer.putShort(result.appliedHeight.toShort())
        buffer.putShort(result.appliedFps.toShort())
        buffer.putShort(0)
        return buffer.array()
    }

    fun decodeQualityResult(payload: ByteArray): QualityResult {
        require(payload.size == 14) { "quality result payload must be 14 bytes" }
        val buffer = ByteBuffer.wrap(payload).order(ByteOrder.LITTLE_ENDIAN)
        val requestId = buffer.int.toLong() and 0xffffffffL
        val status = buffer.get().toInt() and 0xff
        val reason = buffer.get().toInt() and 0xff
        val width = buffer.short.toInt() and 0xffff
        val height = buffer.short.toInt() and 0xffff
        val fps = buffer.short.toInt() and 0xffff
        return QualityResult(requestId, status, reason, width, height, fps)
    }

    /** Paused and CommitQuality share the epoch frame shape. */
    fun encodeEpoch(epoch: Epoch): ByteArray {
        val buffer = ByteBuffer.allocate(8).order(ByteOrder.LITTLE_ENDIAN)
        buffer.putInt(epoch.requestId.toInt())
        buffer.putInt(epoch.streamEpoch.toInt())
        return buffer.array()
    }

    fun decodeEpoch(payload: ByteArray): Epoch {
        require(payload.size == 8) { "epoch payload must be 8 bytes" }
        val buffer = ByteBuffer.wrap(payload).order(ByteOrder.LITTLE_ENDIAN)
        val requestId = buffer.int.toLong() and 0xffffffffL
        val streamEpoch = buffer.int.toLong() and 0xffffffffL
        return Epoch(requestId, streamEpoch)
    }

    data class Header(
        val type: Byte,
        val flags: Short,
        val sequence: Int,
        val payloadLength: Int,
        val ptsUs: Long,
    )

    /** Both transport readers share exact type-specific limits BEFORE allocation. */
    fun readHostFrame(input: InputStream): Pair<Header, ByteArray> = readFrame(input, CLIENT_AUTH_MAX_BYTES) { type ->
        when (type) {
            TYPE_PING -> 8..8
            TYPE_VIDEO_IDR_REQUEST -> 0..0
            TYPE_PREPARE_QUALITY -> 14..14
            TYPE_COMMIT_QUALITY -> 8..8
            TYPE_CLIENT_AUTH -> CLIENT_AUTH_MIN_BYTES..CLIENT_AUTH_MAX_BYTES
            else -> error("unexpected host message type 0x%02x".format(type))
        }
    }

    fun readFrame(input: InputStream, maxPayloadLength: Int = MAX_PAYLOAD, payloadBounds: ((Byte) -> IntRange)? = null): Pair<Header, ByteArray> {
        require(maxPayloadLength in 0..MAX_PAYLOAD) { "invalid caller payload limit" }
        val headerBytes = ByteArray(HEADER_SIZE)
        readExact(input, headerBytes)
        val b = ByteBuffer.wrap(headerBytes).order(ByteOrder.LITTLE_ENDIAN)
        require(b.int == MAGIC) { "AMB wire magic mismatch" }
        require(b.get() == VERSION) { "unsupported AMB wire version" }
        val type = b.get()
        val flags = b.short
        val sequence = b.int
        val payloadLength = b.int
        val ptsUs = b.long
        require(payloadLength in 0..maxPayloadLength) { "invalid payload length: $payloadLength" }
        payloadBounds?.let { require(payloadLength in it(type)) { "unexpected control payload length: $payloadLength" } }
        val payload = ByteArray(payloadLength)
        readExact(input, payload)
        return Header(type, flags, sequence, payloadLength, ptsUs) to payload
    }

    fun writeFrame(output: OutputStream, header: Header, payload: ByteArray) {
        require(payload.size == header.payloadLength)
        require(payload.size <= MAX_PAYLOAD)
        val b = ByteBuffer.allocate(HEADER_SIZE).order(ByteOrder.LITTLE_ENDIAN)
        b.putInt(MAGIC)
        b.put(VERSION)
        b.put(header.type)
        b.putShort(header.flags)
        b.putInt(header.sequence)
        b.putInt(payload.size)
        b.putLong(header.ptsUs)
        synchronized(output) {
            output.write(b.array())
            if (payload.isNotEmpty()) output.write(payload)
            output.flush()
        }
    }

    private fun readExact(input: InputStream, target: ByteArray) {
        var offset = 0
        while (offset < target.size) {
            val n = input.read(target, offset, target.size - offset)
            if (n < 0) throw java.io.EOFException("accessory stream closed")
            if (n == 0) continue
            offset += n
        }
    }
}
