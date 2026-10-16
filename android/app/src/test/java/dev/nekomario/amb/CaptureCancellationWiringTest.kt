package dev.nekomario.amb

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

/**
 * Pins the Activity's cancellation wiring, which the behavioural tests cannot
 * reach: those pass a look-alike sink, so reverting `MainActivity` to
 * `handler.post { stopCamera() }` — the AUTHREV-001 defect — used to leave every
 * test green (REV-002).
 */
class CaptureCancellationWiringTest {
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
    fun the_activity_owns_capture_cancellation_and_never_posts_it_to_the_main_looper() {
        val source = mainActivitySource()
        assertTrue(
            "MainActivity must own exactly one production CaptureCancellation",
            source.contains("private val captureCancellation = CaptureCancellation"),
        )
        assertTrue("MainActivity must not keep a second ad-hoc teardown executor",
            !source.contains("mwb-capture-teardown"))
        val cancellationPath = bodyOf(source, "private fun onCaptureCancelled(")
        assertTrue("the cancellation path must delegate to the owner",
            cancellationPath.contains("captureCancellation.cancel"))
        for (forbidden in listOf("handler.post", "postDelayed", "runOnUiThread", "Looper")) {
            assertFalse(
                "onCaptureCancelled must not route the producer stop through $forbidden (AUTHREV-001)",
                cancellationPath.contains(forbidden),
            )
        }
    }

    @Test
    fun the_owner_is_shut_down_with_the_activity() {
        val source = mainActivitySource()
        assertTrue("onDestroy must shut the teardown thread down",
            bodyOf(source, "override fun onDestroy()").contains("captureCancellation.shutdown()"))
    }
}
