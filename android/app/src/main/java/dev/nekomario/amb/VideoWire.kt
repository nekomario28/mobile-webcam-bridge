package dev.nekomario.amb

import java.nio.ByteBuffer
import java.nio.ByteOrder

object VideoWire {
    const val CODEC_H264: Byte = 1
    const val NAL_FORMAT_ANNEX_B: Byte = 1
    const val CONFIG_SIZE = 16

    fun encodeConfig(width: Int, height: Int, fps: Int, bitrate: Int): ByteArray {
        require(width in 1..0xffff)
        require(height in 1..0xffff)
        require(fps in 1..0xffff)
        require(bitrate > 0)
        return ByteBuffer.allocate(CONFIG_SIZE)
            .order(ByteOrder.LITTLE_ENDIAN)
            .apply {
                putShort(width.toShort())
                putShort(height.toShort())
                putShort(fps.toShort())
                putShort(0)
                putInt(bitrate)
                put(CODEC_H264)
                put(NAL_FORMAT_ANNEX_B)
                putShort(0)
            }
            .array()
    }

    fun joinCodecConfig(config: H264Encoder.CodecConfig): ByteArray {
        val a = config.csd0 ?: ByteArray(0)
        val b = config.csd1 ?: ByteArray(0)
        return ByteArray(a.size + b.size).also {
            System.arraycopy(a, 0, it, 0, a.size)
            System.arraycopy(b, 0, it, a.size, b.size)
        }
    }
}
