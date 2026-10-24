package dev.nekomario.amb

/**
 * Edition seam: which capture modes a build may advertise and apply.
 *
 * Public OSS and the Play free tier use [OssCap1080p30]. The private Play Pro
 * fork replaces [active] at process start so entitlement can raise the cap
 * (4K, etc.) without forking the capture pipeline.
 *
 * Apache-2.0 still lets someone rebuild from source with a different policy;
 * the seam only sets the default of the binaries we ship.
 */
fun interface EditionPolicy {
    fun allows(width: Int, height: Int, fps: Int): Boolean

    fun allows(mode: CaptureModes.CaptureMode): Boolean =
        allows(mode.width, mode.height, mode.targetFps)

    fun filter(modes: List<CaptureModes.CaptureMode>): List<CaptureModes.CaptureMode> =
        modes.filter(::allows)

    companion object {
        /**
         * Long edge ≤ 1920, short edge ≤ 1080, fps ≤ 30 (covers landscape and
         * portrait 1080p). Higher resolutions or higher fps are Pro-only in
         * shipped builds.
         */
        val OssCap1080p30: EditionPolicy = EditionPolicy { width, height, fps ->
            val longEdge = maxOf(width, height)
            val shortEdge = minOf(width, height)
            longEdge in 1..1920 && shortEdge in 1..1080 && fps in 1..30
        }

        /** No resolution/fps gate (private Pro unlock / self-built uncapped). */
        val Uncapped: EditionPolicy = EditionPolicy { width, height, fps ->
            width > 0 && height > 0 && fps > 0
        }

        /** Process-wide policy. Private fork may assign before any capture. */
        @JvmField
        @Volatile
        var active: EditionPolicy = OssCap1080p30
    }
}
