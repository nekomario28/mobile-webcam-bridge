package dev.nekomario.amb

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The Stop action as a behaviour rather than as text in the Activity.
 *
 * `MainActivity` is exported, so the action string is not a credential: only a token minted
 * by this process is accepted, and the notification — same process — is the only thing given
 * one. The test this replaced asserted that the *string* `UUID.randomUUID().toString()`
 * appeared somewhere in `MainActivity.kt`, which stayed green while a different feature used
 * the same expression in the same file and this token could have been a constant
 * (independent review, 2026-10-24, WR3).
 */
class StopActionTest {

    @Test
    fun the_action_and_this_process_token_together_are_the_credential() {
        assertTrue(StopAction.accepts(StopAction.ACTION, StopAction.currentToken()))
    }

    @Test
    fun the_action_alone_is_not_enough() {
        assertFalse(
            "an exported Activity must not accept the action by itself",
            StopAction.accepts(StopAction.ACTION, null),
        )
        assertFalse("an empty extra is not a token", StopAction.accepts(StopAction.ACTION, ""))
        assertFalse(
            "another app's action name is not this one",
            StopAction.accepts("dev.example.STOP_SESSION", StopAction.currentToken()),
        )
        assertFalse("no action is not this action", StopAction.accepts(null, StopAction.currentToken()))
    }

    @Test
    fun a_token_from_anywhere_else_is_refused() {
        // What another process would present. A constant, or a value written down in the
        // source, is exactly this case for anyone who can read the file.
        assertFalse(
            "a foreign token must not act on this process's session",
            StopAction.accepts(StopAction.ACTION, StopAction.newToken()),
        )
        assertFalse(
            "a near miss is still a miss",
            StopAction.accepts(StopAction.ACTION, StopAction.currentToken() + " "),
        )
        assertFalse(
            "comparison must be exact, not case-folded",
            StopAction.accepts(StopAction.ACTION, StopAction.currentToken().uppercase()),
        )
    }

    @Test
    fun the_token_is_generated_and_stable_for_the_process() {
        // Generated: two tokens differ, so a constant cannot stand in for one.
        assertNotEquals(
            "a constant would pass every other test here",
            StopAction.newToken(), StopAction.newToken(),
        )
        // Stable: the notification is handed one value and gives the same one back, so
        // regenerating on each call would leave Stop dead.
        assertEquals(StopAction.currentToken(), StopAction.currentToken())
    }
}
