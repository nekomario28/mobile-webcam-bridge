package dev.nekomario.amb

/**
 * What a dropped link should do next. No addresses, names, or raw errors.
 *
 * A refusal (revoked, expired, rejected, storage) is not something retrying
 * can fix, so capture must not resume on its own. A cable pull or a TCP
 * close can: the user already pressed Start camera.
 */
internal object ReconnectPolicy {
    data class Copy(val english: String, val japanese: String)

    fun shouldResume(reason: String?): Boolean {
        if (reason.isNullOrBlank()) return true
        val lower = reason.lowercase()
        return REFUSAL.none { lower.contains(it) }
    }

    /** 1s, 2s, 4s, then 8s. [attempt] is 0-based and saturates. */
    fun rediscoverDelayMs(attempt: Int): Long {
        val shift = attempt.coerceIn(0, 3)
        return 1_000L shl shift
    }

    fun usbReconnecting() = Copy(
        "USB · Reconnecting\nLeave this screen open. If it stays here, unplug and replug the cable, then press Retry.",
        "USB · 再接続中\nこの画面を開いたままにしてください。戻らないときはケーブルを挿し直し、「再試行」を押してください。",
    )

    fun wifiReconnecting() = Copy(
        "Wi-Fi · Reconnecting\nThe PC retries on its own. Keep this screen open, or press Retry.",
        "Wi-Fi · 再接続中\nPCが自動で再試行します。この画面を開いたままにするか、「再試行」を押してください。",
    )

    fun usbRefused() = Copy(
        "USB · Connection refused\nApprove this PC on the phone, or press Retry.",
        "USB · 接続できませんでした\nスマホでこのPCを許可するか、「再試行」を押してください。",
    )

    fun wifiRefused() = Copy(
        "Wi-Fi · Could not connect\nCheck the pairing on the PC, then press Retry.",
        "Wi-Fi · 接続できませんでした\nPC側のペアリングを確認して、「再試行」を押してください。",
    )

    /** `reconnect transport=usb attempt=2 delay_ms=4000` — tokens and numbers only. */
    fun logLine(transport: String, attempt: Int, delayMs: Long): String {
        val label = if (transport == "usb" || transport == "wifi") transport else "unknown"
        val safeAttempt = attempt.coerceAtLeast(0)
        val safeDelay = delayMs.coerceAtLeast(0)
        return "reconnect transport=$label attempt=$safeAttempt delay_ms=$safeDelay"
    }

    private val REFUSAL = listOf("revoked", "expired", "rejected", "denied", "storage")
}
