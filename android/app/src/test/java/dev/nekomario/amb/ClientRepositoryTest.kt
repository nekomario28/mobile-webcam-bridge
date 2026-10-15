package dev.nekomario.amb

import org.junit.Assert.*
import org.junit.Test

class ClientRepositoryTest {
    private val now = 1_800_000_000L
    private fun key(): ByteArray {
        val generator = java.security.KeyPairGenerator.getInstance("EC")
        generator.initialize(java.security.spec.ECGenParameterSpec("secp256r1"))
        return generator.generateKeyPair().public.encoded.takeLast(65).toByteArray()
    }
    private fun entry(key: ByteArray = key(), label: String = "studio-pc") =
        ClientAuthorization.Client(ClientAuthorization.deriveClientId(key), key, label,
            now, now + ClientAuthorization.EXPIRY_SECONDS)

    @Test fun approval_is_persisted_and_survives_recreation() {
        var disk: ByteArray? = null
        val repository = ClientRepository({ disk }, { disk = it.copyOf() })
        val publicKey = key()
        val id = ClientAuthorization.deriveClientId(publicKey)
        repository.approve(id, publicKey, "撮影PC", now)
        assertNotNull(disk)
        val loaded = ClientRepository({ disk }, { fail("load must not write") }).snapshot().single()
        assertEquals("撮影PC", loaded.label)
        assertArrayEquals(publicKey, loaded.publicKey)
        assertArrayEquals(id, loaded.clientId)
        assertEquals(now + ClientAuthorization.EXPIRY_SECONDS, loaded.expiresAtUnixSeconds)
    }

    @Test fun key_bytes_cannot_be_changed_through_input_or_snapshot_aliases() {
        val publicKey = key()
        val original = publicKey.copyOf()
        val id = ClientAuthorization.deriveClientId(publicKey)
        val expectedId = id.copyOf()
        val client = entry(publicKey)
        val repository = ClientRepository({ null }, {})
        repository.approve(id, publicKey, "studio-pc", now)
        publicKey.fill(0); id.fill(0)
        repository.snapshot().single().publicKey.fill(0)
        repository.snapshot().single().clientId.fill(0)
        assertArrayEquals(original, repository.snapshot().single().publicKey)
        assertArrayEquals(expectedId, repository.snapshot().single().clientId)
        assertArrayEquals(original, client.publicKey)
    }

    @Test fun failed_approval_latches_the_repository_closed() {
        val repository = ClientRepository({ null }, { throw java.io.IOException("disk full") })
        val publicKey = key()
        val id = ClientAuthorization.deriveClientId(publicKey)
        assertThrows(IllegalStateException::class.java) { repository.approve(id, publicKey, "pc", now) }
        assertThrows(IllegalStateException::class.java) { repository.snapshot() }
        assertThrows(IllegalStateException::class.java) { repository.approve(id, publicKey, "pc", now) }
    }

    @Test fun failed_revocation_is_not_reported_as_success_and_stops_further_access() {
        val before = entry()
        val disk = ClientRecordCodec.encode(listOf(before))
        val repository = ClientRepository({ disk }, { throw java.io.IOException("read-only") })
        assertThrows(IllegalStateException::class.java) { repository.revoke(before.clientId) }
        assertThrows(IllegalStateException::class.java) { repository.snapshot() }
        // A failed revoke is not a claim that the old on-disk grant disappeared.
        assertEquals(listOf(before), ClientRecordCodec.decode(disk))
    }

    @Test fun explicit_renewal_and_revocation_are_persisted() {
        val before = entry()
        var disk = ClientRecordCodec.encode(listOf(before))
        val repository = ClientRepository({ disk }, { disk = it.copyOf() })
        repository.renew(before.clientId, now + 50)
        var loaded = ClientRepository({ disk }, {}).snapshot()
        assertEquals(now + 50, loaded.single().approvedAtUnixSeconds)
        assertEquals(now + 50 + ClientAuthorization.EXPIRY_SECONDS, loaded.single().expiresAtUnixSeconds)
        repository.revoke(before.clientId)
        loaded = ClientRepository({ disk }, {}).snapshot()
        assertTrue(loaded.isEmpty())
        repository.revoke(before.clientId) // idempotent, not a second approval
        assertThrows(IllegalArgumentException::class.java) { repository.renew(before.clientId, now) }
    }

    @Test fun malformed_existing_storage_never_becomes_a_successful_empty_list() {
        val good = ClientRecordCodec.encode(listOf(entry()))
        val bad = (good.indices.map { good.copyOf(it) } + listOf(
            byteArrayOf(2, 0), byteArrayOf(1, 9), good + byteArrayOf(0),
            ByteArray(ClientRecordCodec.MAX_BYTES + 1),
        ))
        for (bytes in bad) {
            assertThrows(Exception::class.java) {
                ClientRepository({ bytes }, { fail("corrupt storage must not be replaced") })
            }
        }
    }

    @Test fun bad_public_keys_duplicates_and_periods_cannot_be_encoded() {
        val good = entry()
        for (clients in listOf(
            listOf(good, good),
            List(9) { good },
            listOf(ClientAuthorization.Client(ByteArray(16), good.publicKey, "pc", now, now + 1)),
            listOf(ClientAuthorization.Client(good.clientId, good.publicKey, "pc", now, now)),
            listOf(ClientAuthorization.Client(good.clientId, good.publicKey, "pc", now, now + ClientAuthorization.EXPIRY_SECONDS + 1)),
            listOf(ClientAuthorization.Client(good.clientId, good.publicKey, "pc", -1, 10)),
        )) assertThrows(IllegalArgumentException::class.java) { ClientRecordCodec.encode(clients) }
        val offCurve = ByteArray(65).also { it[0] = 4 }
        assertFalse(ClientAuthorization.validPublicKey(offCurve))
        assertNull(ClientAuthorization.approve(emptyList(), ClientAuthorization.deriveClientId(offCurve), offCurve, "pc", now))
    }

    @Test fun strict_utf8_and_point_validation_apply_on_decode_too() {
        val good = ClientRecordCodec.encode(listOf(entry(label = "aa")))
        val badLabel = good.copyOf().also { it[68] = 0xc0.toByte(); it[69] = 0xaf.toByte() }
        assertThrows(Exception::class.java) { ClientRecordCodec.decode(badLabel) }
        val badPoint = good.copyOf().also { for (i in 3 until 67) it[i] = 0 }
        assertThrows(Exception::class.java) { ClientRecordCodec.decode(badPoint) }
    }

    @Test fun labels_are_bounded_in_utf8_bytes_not_characters_and_never_silently_truncated() {
        val publicKey = key()
        val id = ClientAuthorization.deriveClientId(publicKey)
        val allowed = "日".repeat(10)
        assertEquals(allowed, ClientAuthorization.approve(emptyList(), id, publicKey, allowed, now)!!.single().label)
        assertTrue(ClientAuthorization.validLabel("camera-猫\uD83D\uDE3A"))
        for (label in listOf("日".repeat(11), "a".repeat(33), "", "\nAdmin", "\u202eAdmin", "pc\uDB40\uDC01", "pc\uDB40\uDC20", "pc\u2028", "pc\u2029", "\ud800")) {
            assertNull("reject unsafe/oversize label", ClientAuthorization.approve(emptyList(), id, publicKey, label, now))
        }
    }

    @Test fun id_key_mismatches_and_approval_time_overflow_are_refused() {
        val publicKey = key()
        val id = ClientAuthorization.deriveClientId(publicKey)
        assertNull(ClientAuthorization.approve(emptyList(), ByteArray(16), publicKey, "pc", now))
        assertNull(ClientAuthorization.approve(emptyList(), id, publicKey, "pc", Long.MAX_VALUE))
        assertNull(ClientAuthorization.approve(emptyList(), id, publicKey, "pc", -1))
    }

    @Test fun full_repository_refuses_new_clients_but_allows_explicit_renewal() {
        var disk = ClientRecordCodec.encode(List(8) { entry(label = "pc-$it") })
        val repository = ClientRepository({ disk }, { disk = it.copyOf() })
        val newKey = key()
        assertThrows(IllegalStateException::class.java) {
            repository.approve(ClientAuthorization.deriveClientId(newKey), newKey, "new", now)
        }
        repository.renew(repository.snapshot().first().clientId, now + 1)
        assertEquals(8, ClientRecordCodec.decode(disk).size)
    }

    @Test fun missing_storage_is_not_the_same_as_a_read_error() {
        assertTrue(ClientRepository({ null }, {}).snapshot().isEmpty())
        assertThrows(java.io.IOException::class.java) {
            ClientRepository({ throw java.io.IOException("cannot read") }, { fail("must not write") })
        }
    }
}
