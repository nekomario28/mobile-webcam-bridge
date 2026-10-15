package dev.nekomario.amb

import org.junit.Assert.assertEquals

/** JVM-only key/signature helpers; never shipped in the application. */
object ClientCryptoFixture {
    fun phoneBinding() = ByteArray(Wire.PHONE_BINDING_BYTES) { (it + 32).toByte() }
    fun keyPair(): Pair<ByteArray, java.security.PrivateKey> {
        val generator = java.security.KeyPairGenerator.getInstance("EC")
        generator.initialize(java.security.spec.ECGenParameterSpec("secp256r1"))
        val pair = generator.generateKeyPair()
        val point = (pair.public as java.security.interfaces.ECPublicKey).w
        fun fixed32(value: java.math.BigInteger): ByteArray =
            value.toByteArray().takeLast(32).toByteArray().let { ByteArray(32 - it.size) + it }
        return (byteArrayOf(0x04) + fixed32(point.affineX) + fixed32(point.affineY)) to pair.private
    }

    fun derToRaw(der: ByteArray): ByteArray {
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
            // DER adds a sign byte to keep the unsigned scalar positive;
            // raw encoding restores the fixed 32-byte width with zero padding.
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

    fun sign(key: java.security.PrivateKey, message: ByteArray): ByteArray =
        derToRaw(java.security.Signature.getInstance("SHA256withECDSA").run {
            initSign(key); update(message); sign()
        })
}
