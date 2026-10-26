package dev.nekomario.amb

/**
 * The short path to a first frame. Copy only: no addresses, keys, or device names.
 *
 * The PC takes the Wi-Fi pairing over the USB cable, so the phone does not ask
 * the user to type a fingerprint. These strings are the one next action, not a
 * second copy of the procedure that lives under Details.
 */
internal object SetupGuide {
    data class Copy(val english: String, val japanese: String)

    /** No accessory yet. The PC has to press Connect; the phone cannot start AOA. */
    fun usbWaitingForHost() = Copy(
        "USB · Next: press Connect on the PC",
        "USB · 次: PCで「接続」を押してください",
    )

    /** The cable hand-off worked. Two taps remain, and both are already on screen. */
    fun pairingSent() = Copy(
        "Wi-Fi pairing sent.\nNext: tap Wi-Fi here, and press Switch to Wi-Fi on the PC.",
        "Wi-Fi設定を送りました。\n次: ここでWi-Fiを押し、PCで「Wi-Fiに切り替える」を押してください。",
    )
}
