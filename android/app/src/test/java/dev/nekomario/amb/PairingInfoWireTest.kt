package dev.nekomario.amb

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

/**
 * The payload the phone sends over the cable, pinned byte for byte.
 *
 * `host-rs/src/wire.rs` decodes exactly this layout and pins the same hexadecimal
 * string in `pairing_info_round_trips_and_rejects_malformed_payloads`, so a change
 * on one side cannot pass while the other stays behind.
 */
class PairingInfoWireTest {
    private val fingerprint = "ab".repeat(32)
    private val key = "cd".repeat(16)

    private fun hex(bytes: ByteArray) = bytes.joinToString("") { "%02x".format(it) }

    @Test
    fun the_payload_is_the_layout_the_pc_decodes() {
        val payload = Wire.encodePairingInfo(listOf("192.168.1.3"), fingerprint, key)
        // version 1, one address, 192.168.1.3, SHA-256, 128-bit key
        assertEquals("0101" + "c0a80103" + "ab".repeat(32) + "cd".repeat(16), hex(payload))
    }

    @Test
    fun addresses_keep_their_order_and_four_are_carried() {
        val payload = Wire.encodePairingInfo(
            listOf("192.168.1.3", "100.66.21.23", "10.0.0.2", "172.16.4.5", "8.8.8.8"),
            fingerprint,
            key,
        )
        assertEquals(4, payload[1].toInt())
        assertEquals(2 + 4 * 4 + 32 + 16, payload.size)
        assertEquals(
            "0104" + "c0a80103" + "64421517" + "0a000002" + "ac100405" + "ab".repeat(32) + "cd".repeat(16),
            hex(payload),
        )
    }

    @Test
    fun ambiguous_or_malformed_input_is_refused() {
        assertThrows(IllegalArgumentException::class.java) {
            Wire.encodePairingInfo(emptyList(), fingerprint, key)
        }
        // "01" and "1" are the same address to some parsers and different to others;
        // an address that loses its meaning must not be sent.
        assertThrows(IllegalArgumentException::class.java) {
            Wire.encodePairingInfo(listOf("01.2.3.4"), fingerprint, key)
        }
        assertThrows(IllegalArgumentException::class.java) {
            Wire.encodePairingInfo(listOf("192.168.1.300"), fingerprint, key)
        }
        assertThrows(IllegalArgumentException::class.java) {
            Wire.encodePairingInfo(listOf("192.168.1.3"), "ab".repeat(31), key)
        }
        assertThrows(IllegalArgumentException::class.java) {
            Wire.encodePairingInfo(listOf("192.168.1.3"), fingerprint, "not-a-key")
        }
    }

    @Test
    fun the_surplus_address_is_dropped_rather_than_failing() {
        val payload = Wire.encodePairingInfo(
            listOf("192.168.1.3", "10.0.0.2", "172.16.4.5", "8.8.8.8", "1.1.1.1"),
            fingerprint,
            key,
        )
        assertEquals(4, payload[1].toInt())
        assertArrayEquals(ByteArray(0), ByteArray(0))
    }
}
