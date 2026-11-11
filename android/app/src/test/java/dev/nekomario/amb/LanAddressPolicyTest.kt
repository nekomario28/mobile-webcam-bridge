package dev.nekomario.amb

import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class LanAddressPolicyTest {
    private fun ip(literal: String): InetAddress = InetAddress.getByName(literal)

    @Test
    fun localNetworksAreAcceptedOnWifiHotspotAndTetheringInterfaces() {
        for ((address, name) in listOf(
            "192.168.1.23" to "wlan0",
            "10.0.0.5" to "wlan0",
            "172.20.3.4" to "eth0",
            "192.168.43.1" to "ap0",
            "192.168.42.129" to "rndis0",
            "169.254.10.2" to "wlan1",
            "fe80::1" to "wlan0",
            "fd12:3456::7" to "wlan0",
            "10.0.2.15" to "eth0", // emulator
        )) {
            assertTrue("$address on $name", LanAddressPolicy.acceptsLocal(ip(address)) { name })
        }
    }

    @Test
    fun loopbackIsAcceptedForAdbForwarding() {
        assertTrue(LanAddressPolicy.acceptsLocal(ip("127.0.0.1")) { "lo" })
        assertTrue(LanAddressPolicy.acceptsLocal(ip("::1")) { "lo" })
    }

    @Test
    fun publicAndCarrierAddressesAreRefused() {
        for (address in listOf("100.64.1.2", "8.8.8.8", "2001:db8::1", "2607:f8b0::1", "192.0.0.4")) {
            assertFalse(address, LanAddressPolicy.acceptsLocal(ip(address)) { "wlan0" })
        }
        assertFalse(LanAddressPolicy.acceptsLocal(null))
    }

    @Test
    fun privateAddressesOnMobileDataInterfacesAreRefused() {
        for (name in listOf("rmnet_data0", "rmnet0", "ccmni1", "v4-rmnet_data2", "seth_lte0", "wwan0", "r_rmnet_data1")) {
            assertFalse(name, LanAddressPolicy.acceptsLocal(ip("10.12.0.4")) { name })
        }
        // An interface that cannot be identified is not assumed to be cellular.
        assertTrue(LanAddressPolicy.acceptsLocal(ip("192.168.1.2")) { null })
    }

    @Test
    fun cellularNamesAreRecognisedWithoutCatchingWifiOrTethering() {
        for (name in listOf("RMNET_DATA0", "ccmni0", "pdp0", "qmimux0")) {
            assertTrue(name, LanAddressPolicy.isCellularName(name))
        }
        for (name in listOf("wlan0", "ap0", "swlan0", "rndis0", "eth0", "v4-wlan0", "tun0", null)) {
            assertFalse(name.toString(), LanAddressPolicy.isCellularName(name))
        }
    }

    @Test
    fun anAcceptedLoopbackSocketReportsAnAcceptableLocalAddress() {
        // What `acceptLoop` actually sees: the socket's local address after accept.
        ServerSocket(0, 1, ip("127.0.0.1")).use { server ->
            Socket("127.0.0.1", server.localPort).use {
                server.accept().use { accepted ->
                    assertEquals(ip("127.0.0.1"), accepted.localAddress)
                    assertTrue(LanAddressPolicy.acceptsLocal(accepted.localAddress))
                }
            }
        }
    }
}
