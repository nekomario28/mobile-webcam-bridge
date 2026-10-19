package dev.nekomario.amb

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

/**
 * Pins the lifecycle rule that makes the phone usable as a webcam while it is
 * being used as a phone.
 *
 * Measured on a physical phone (2026-10-19): pressing Home closed the session and
 * the consumer read 0 frames, because `onPause` stopped the camera. The fix is a
 * foreground service around the camera plus "keep capturing while backgrounded",
 * and both halves are invisible to the behavioural tests — they drive the session
 * directly instead of the Activity. A revert to the old `onPause` would leave
 * every other test green, which is exactly what this file refuses.
 */
class SessionServiceWiringTest {
    private fun mainActivitySource(): String {
        val roots = generateSequence(File(checkNotNull(System.getProperty("user.dir")))) { it.parentFile }
        val candidates = roots.flatMap { root ->
            listOf(
                File(root, "android/app/src/main/java/dev/nekomario/amb/MainActivity.kt"),
                File(root, "app/src/main/java/dev/nekomario/amb/MainActivity.kt"),
            ).asSequence()
        }
        return candidates.firstOrNull { it.isFile }?.readText()
            ?: error("MainActivity.kt not found from ${System.getProperty("user.dir")}")
    }

    /** Body of the first function whose declaration contains [signature]. */
    private fun bodyOf(source: String, signature: String): String {
        val declaration = source.indexOf(signature)
        check(declaration >= 0) { "missing $signature" }
        val open = source.indexOf('{', declaration)
        check(open > 0) { "missing body for $signature" }
        var depth = 0
        var index = open
        while (index < source.length) {
            when (source[index]) {
                '{' -> depth++
                '}' -> {
                    depth--
                    if (depth == 0) return source.substring(open + 1, index)
                }
            }
            index++
        }
        error("unbalanced braces after $signature")
    }

    @Test
    fun the_camera_runs_inside_the_foreground_service() {
        val source = mainActivitySource()
        val start = bodyOf(source, "private fun startCamera(")
        assertTrue("starting the camera must put the session in the foreground",
            start.contains("SessionService.start(this)"))
        val bridgeIndex = start.indexOf("CameraBridgeController(")
        val serviceIndex = start.indexOf("SessionService.start(this)")
        assertTrue("the service must be started before the camera opens",
            serviceIndex in 0 until bridgeIndex)
        val stop = bodyOf(source, "private fun stopCamera(")
        assertTrue("stopping the camera must end the foreground session",
            stop.contains("SessionService.stop(this)"))
    }

    @Test
    fun leaving_the_foreground_keeps_streaming_but_hides_the_secret() {
        val pause = bodyOf(mainActivitySource(), "override fun onPause()")
        assertFalse(
            "onPause must not stop the camera: that is the defect this pins (measured: 0 frames after Home)",
            pause.contains("stopCamera("),
        )
        assertTrue("the pairing key must still be hidden when the app leaves the foreground",
            pause.contains("pairInfo.visibility = View.GONE"))
        assertTrue("the secure flag must still be cleared when the app leaves the foreground",
            pause.contains("FLAG_SECURE"))
    }

    @Test
    fun continued_capture_is_visible_to_the_user() {
        // A foreground service whose notification is hidden tells the user nothing:
        // measured on a physical phone, the service ran with the camera type and no
        // notification appeared, because POST_NOTIFICATIONS was never asked for.
        val source = mainActivitySource()
        val manifest = generateSequence(File(checkNotNull(System.getProperty("user.dir")))) { it.parentFile }
            .flatMap { root ->
                listOf(
                    File(root, "android/app/src/main/AndroidManifest.xml"),
                    File(root, "app/src/main/AndroidManifest.xml"),
                    File(root, "src/main/AndroidManifest.xml"),
                ).asSequence()
            }
            .firstOrNull { it.isFile }
            ?.readText()
            ?: error("AndroidManifest.xml not found from ${System.getProperty("user.dir")}")
        assertTrue("the manifest must declare which foreground service type it uses",
            manifest.contains("foregroundServiceType=\"camera\""))
        assertTrue("the manifest must request POST_NOTIFICATIONS for the session notification",
            manifest.contains("android.permission.POST_NOTIFICATIONS"))
        assertTrue("the camera must ask for the notification permission before it starts the session",
            bodyOf(source, "private fun startCamera(").contains("requestNotificationPermissionIfNeeded()"))
    }

    @Test
    fun the_notification_stop_is_not_blocked_from_the_background() {
        // A service that answers the action by starting an Activity is blocked by
        // the background activity-start restriction on Android 10+, so Stop would
        // silently do nothing; a notification's activity PendingIntent is exempt.
        val service = generateSequence(File(checkNotNull(System.getProperty("user.dir")))) { it.parentFile }
            .flatMap { root ->
                listOf(
                    File(root, "android/app/src/main/java/dev/nekomario/amb/SessionService.kt"),
                    File(root, "app/src/main/java/dev/nekomario/amb/SessionService.kt"),
                ).asSequence()
            }
            .firstOrNull { it.isFile }
            ?.readText()
            ?: error("SessionService.kt not found")
        assertTrue("the stop action must start the Activity the user exempts",
            service.contains("PendingIntent.getActivity("))
        assertFalse("a service PendingIntent would have to start an activity itself",
            service.contains("PendingIntent.getService("))
        assertFalse("the service must not start the activity from a background callback",
            service.contains("startActivity("))
    }

    @Test
    fun the_notification_can_stop_the_session() {
        val source = mainActivitySource()
        assertTrue("the activity must handle the notification's stop action",
            source.contains("ACTION_STOP_SESSION"))
        assertTrue("the stop action must stop the camera",
            bodyOf(source, "override fun onResume()").contains("stopCamera("))
        assertTrue("the service must be released with the activity",
            bodyOf(source, "override fun onDestroy()").contains("SessionService.stop(this)"))
    }
}
