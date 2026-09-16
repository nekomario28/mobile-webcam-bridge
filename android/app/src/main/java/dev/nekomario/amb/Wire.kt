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
    const val TYPE_VIDEO_CONFIG: Byte = 0x10
    const val TYPE_VIDEO_AU: Byte = 0x11
    const val TYPE_VIDEO_IDR_REQUEST: Byte = 0x12
    const val TYPE_STATS: Byte = 0x20
    const val TYPE_ERROR: Byte = 0x7f

    const val VIDEO_FLAG_KEYFRAME: Short = 0x0001
    const val VIDEO_FLAG_CONFIG_INCLUDED: Short = 0x0002
    const val VIDEO_FLAG_DISCONTINUITY: Short = 0x0004

    data class Header(
        val type: Byte,
        val flags: Short,
        val sequence: Int,
        val payloadLength: Int,
        val ptsUs: Long,
    )

    fun readFrame(input: InputStream): Pair<Header, ByteArray> {
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
        require(payloadLength in 0..MAX_PAYLOAD) { "invalid payload length: $payloadLength" }
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
