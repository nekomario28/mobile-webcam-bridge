package dev.nekomario.amb

import java.nio.ByteBuffer
import java.nio.ByteOrder

/** Normalizes common MediaCodec AVC output into Annex-B access units. */
object H264AnnexB {
    private val startCode = byteArrayOf(0, 0, 0, 1)

    fun normalize(data: ByteArray): ByteArray {
        if (data.isEmpty()) return data
        if (hasStartCode(data, 0)) return data

        // Android AVC encoders that do not emit Annex-B commonly use four-byte
        // big-endian NAL lengths. Reject ambiguous/corrupt input instead of
        // forwarding it as if it were valid H.264.
        val out = java.io.ByteArrayOutputStream(data.size + 32)
        var offset = 0
        while (offset < data.size) {
            require(data.size - offset >= 4) { "truncated AVC length prefix" }
            val length = ByteBuffer.wrap(data, offset, 4)
                .order(ByteOrder.BIG_ENDIAN)
                .int
            require(length > 0) { "invalid AVC NAL length: $length" }
            offset += 4
            require(length <= data.size - offset) { "AVC NAL length exceeds access unit" }
            out.write(startCode)
            out.write(data, offset, length)
            offset += length
        }
        return out.toByteArray()
    }

    private fun hasStartCode(data: ByteArray, offset: Int): Boolean {
        if (data.size - offset >= 4 &&
            data[offset] == 0.toByte() &&
            data[offset + 1] == 0.toByte() &&
            data[offset + 2] == 0.toByte() &&
            data[offset + 3] == 1.toByte()
        ) return true
        return data.size - offset >= 3 &&
            data[offset] == 0.toByte() &&
            data[offset + 1] == 0.toByte() &&
            data[offset + 2] == 1.toByte()
    }
}
