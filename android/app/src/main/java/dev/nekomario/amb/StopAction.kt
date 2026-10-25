package dev.nekomario.amb

import java.util.UUID

/**
 * The one rule behind the foreground notification's Stop action.
 *
 * `MainActivity` is exported, so the action string on its own proves nothing: any installed
 * app can start an explicit component with any action, and without this check it could end a
 * live stream. The credential is a token generated once per process and handed back only by
 * the notification's activity `PendingIntent`, which lives in the same process.
 *
 * Both halves — generating the token and deciding whether an intent may act — live here so a
 * test can drive them. The test this replaced asserted that the *string*
 * `UUID.randomUUID().toString()` appeared somewhere in `MainActivity`, which stayed green
 * while `usbPermissionRequestId` used the same expression elsewhere in the file and the
 * token could have been reverted to a constant (independent review, 2026-10-24, WR3).
 */
internal object StopAction {
    const val ACTION = "dev.nekomario.amb.STOP_SESSION"
    const val EXTRA_TOKEN = "dev.nekomario.amb.STOP_TOKEN"

    /**
     * Generated once per process, and deliberately not written down anywhere: the notification
     * hands this value back, and no other process ever sees it. A UUIDv4 carries 122 random
     * bits, so presenting the right action and guessing the token are different problems.
     */
    private val processToken: String = newToken()

    /** A fresh token. Two calls must differ, or a constant would pass for a credential. */
    fun newToken(): String = UUID.randomUUID().toString()

    /** The token this process accepts, and the one the notification is given. */
    fun currentToken(): String = processToken

    /**
     * Whether an incoming intent may stop the session: the action must match **and** the
     * presented token must equal this process's own. A missing extra (null) and an empty
     * string are refused; so is a token minted by another process, which is what a constant
     * or a persisted value would effectively be.
     */
    fun accepts(action: String?, presentedToken: String?): Boolean =
        action == ACTION && presentedToken != null && presentedToken == processToken
}
