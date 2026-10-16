package dev.nekomario.amb

import android.content.Context
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraManager
import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaCodecList
import android.media.MediaFormat

/**
 * Supported capture modes and the resolution that owns the fixed 720p/30 rule.
 *
 * One mode is one camera × one named encoder × one size/fps/AE-range/bitrate.
 * The mode list intersects the camera's MediaCodec output sizes, the
 * normal-session AE ranges, each size's minimum frame duration, and the
 * selected encoder's alignment and bitrate constraints. An explicit request
 * matches exactly or is rejected — never silently substituted.
 */
object CaptureModes {
    /** Integer fps candidates a webcam mode can target. */
    val FPS_CANDIDATES = listOf(15, 24, 25, 30, 50, 60)

    /** Reference the auto preference optimizes toward: the historical mode. */
    private const val AUTO_WIDTH = 1280
    private const val AUTO_HEIGHT = 720
    private const val AUTO_FPS = 30
    private const val AUTO_BITRATE = 6_000_000

    /** A resolved capture mode. */
    data class CaptureMode(
        val cameraId: String,
        val codecName: String,
        val width: Int,
        val height: Int,
        val targetFps: Int,
        val aeRange: FpsRange,
        val bitrate: Int,
        val minFrameDurationKnown: Boolean,
    )

    /**
     * An AE target fps range. Plain data on purpose: the resolver core stays
     * free of Android types so capability fixtures test it without a device
     * (android.util.Range throws in JVM unit tests). Converted at the Camera2
     * boundary.
     */
    data class FpsRange(val lower: Int, val upper: Int)

    /** A MediaCodec output size with the camera-side minimum frame duration. */
    data class SizeFps(
        val width: Int,
        val height: Int,
        /** Null when the device does not report a minimum frame duration. */
        val minFrameDurationNanos: Long?,
    )

    data class SizeRate(val width: Int, val height: Int, val fps: Int)

    /** The device-side capability shape the resolver consumes. */
    data class Capabilities(
        val cameraId: String,
        val codecName: String,
        val sizes: List<SizeFps>,
        val aeRanges: List<FpsRange>,
        val widthAlignment: Int = 1,
        val heightAlignment: Int = 1,
        val bitrateRange: IntRange = 1..100_000_000,
        /** Null only for fixtures without encoder constraints; collectors supply this set. */
        val encoderSizeRates: Set<SizeRate>? = null,
    )

    /** The PC-side saved preference; any null field is resolved automatically. */
    data class CapturePreference(
        val width: Int? = null,
        val height: Int? = null,
        val fps: Int? = null,
    ) {
        val isAuto: Boolean get() = width == null && height == null && fps == null
    }

    sealed interface Resolution {
        data class Resolved(val mode: CaptureMode) : Resolution
        data class Rejected(val reason: String) : Resolution
    }

    /** Every supported (size, fps) mode for these capabilities. */
    fun supported(caps: Capabilities): List<CaptureMode> {
        val modes = mutableListOf<CaptureMode>()
        require(caps.widthAlignment > 0 && caps.heightAlignment > 0)
        for (size in caps.sizes) {
            if (size.width !in 1..3840 || size.height !in 1..2160) continue
            if (size.width % caps.widthAlignment != 0 || size.height % caps.heightAlignment != 0) {
                continue
            }
            // A known minimum frame duration caps the achievable fps at this
            // size; an unknown one neither includes nor excludes the mode.
            val maxFps = size.minFrameDurationNanos
                ?.takeIf { it > 0 }
                ?.let { (1_000_000_000L / it).toInt() }
            for (fps in FPS_CANDIDATES) {
                if (maxFps != null && fps > maxFps) continue
                if (caps.encoderSizeRates?.contains(SizeRate(size.width, size.height, fps)) == false) continue
                val aeRange = chooseAeRange(caps.aeRanges, fps) ?: continue
                modes.add(
                    CaptureMode(
                        cameraId = caps.cameraId,
                        codecName = caps.codecName,
                        width = size.width,
                        height = size.height,
                        targetFps = fps,
                        aeRange = aeRange,
                        bitrate = autoBitrate(size.width, size.height, fps, caps.bitrateRange),
                        minFrameDurationKnown = maxFps != null,
                    ),
                )
            }
        }
        return modes
    }

    /**
     * The AE range for a candidate fps: only ranges whose upper bound equals
     * the candidate (a 15–30 range cannot promise 24), fixed ranges first,
     * then the narrowest.
     */
    fun chooseAeRange(ranges: List<FpsRange>, fps: Int): FpsRange? =
        ranges.filter { it.upper == fps }
            .minWithOrNull(
                compareBy<FpsRange>(
                    { if (it.lower == fps) 0 else 1 },
                    { it.upper - it.lower },
                ),
            )

    /**
     * Bitrate scaled from the 6 Mbps 720p/30 baseline by pixels × target fps,
     * clamped to the selected encoder's range. Integer math lives only here.
     */
    fun autoBitrate(width: Int, height: Int, fps: Int, range: IntRange): Int {
        val scaled = AUTO_BITRATE.toLong() * width.toLong() * height * fps /
            (AUTO_WIDTH.toLong() * AUTO_HEIGHT * AUTO_FPS)
        return scaled.coerceIn(range.first.toLong(), range.last.toLong()).toInt()
    }

    /** Resolve a preference against the capabilities; exact explicit or auto. */
    fun resolve(caps: Capabilities, preference: CapturePreference): Resolution {
        val modes = supported(caps)
        if (modes.isEmpty()) {
            return Resolution.Rejected("camera ${caps.cameraId} exposes no supported capture mode")
        }
        if (!preference.isAuto) {
            val matched = modes.filter { mode ->
                (preference.width == null || mode.width == preference.width) &&
                    (preference.height == null || mode.height == preference.height) &&
                    (preference.fps == null || mode.targetFps == preference.fps)
            }
            if (matched.isEmpty()) {
                return Resolution.Rejected(
                    "no supported mode matches ${preference.width ?: AUTO_WIDTH}×" +
                        "${preference.height ?: AUTO_HEIGHT} @ ${preference.fps ?: AUTO_FPS} fps",
                )
            }
            return Resolution.Resolved(autoPreferred(matched))
        }
        return Resolution.Resolved(autoPreferred(modes))
    }

    /**
     * Auto preference order: 720p/30 when supported; otherwise the fps closest
     * to 30 from below (the smallest supported fps when none is 30 or below);
     * then the size closest to 720p by aspect ratio, then area; ties by width,
     * height and codec name.
     */
    fun autoPreferred(modes: List<CaptureMode>): CaptureMode {
        val referenceAspect = AUTO_WIDTH.toDouble() / AUTO_HEIGHT
        val referenceArea = AUTO_WIDTH.toLong() * AUTO_HEIGHT
        return modes.minWithOrNull(
            compareBy<CaptureMode>(
                { if (it.targetFps <= AUTO_FPS) kotlin.math.abs(it.targetFps - AUTO_FPS) else Int.MAX_VALUE },
                { if (it.targetFps <= AUTO_FPS) 0 else it.targetFps },
                { kotlin.math.abs(it.width.toDouble() / it.height - referenceAspect) },
                { kotlin.math.abs(it.width.toLong() * it.height - referenceArea) },
                { it.width },
                { it.height },
                { it.codecName },
            ),
        ) ?: error("no capture mode")
    }

    /** Collect the capabilities of one camera with one named encoder. */
    fun collectCapabilities(context: Context, cameraId: String): Capabilities? {
        val manager = context.getSystemService(CameraManager::class.java) ?: return null
        val characteristics = manager.getCameraCharacteristics(cameraId)
        val streamMap = characteristics
            .get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP)
            ?: return null
        val cameraSizes = CameraCatalog.enumerate(context)
            .firstOrNull { it.id == cameraId }
            ?.encoderSizes
            ?: return null
        val encoder = selectAvcSurfaceEncoder() ?: return null
        val videoCapabilities = encoder
            .getCapabilitiesForType(MediaFormat.MIMETYPE_VIDEO_AVC)
            .videoCapabilities
        val sizes = cameraSizes.map { size ->
            // getOutputMinFrameDuration returns 0 when the duration is unknown;
            // such a mode is kept and verified at start, not assumed.
            val duration = streamMap.getOutputMinFrameDuration(MediaCodec::class.java, size)
            SizeFps(
                width = size.width,
                height = size.height,
                minFrameDurationNanos = duration.takeIf { it > 0 },
            )
        }
        return Capabilities(
            cameraId = cameraId,
            codecName = encoder.name,
            sizes = sizes,
            aeRanges = characteristics.get(
                CameraCharacteristics.CONTROL_AE_AVAILABLE_TARGET_FPS_RANGES,
            )?.map { FpsRange(it.lower, it.upper) }.orEmpty(),
            widthAlignment = videoCapabilities.widthAlignment,
            heightAlignment = videoCapabilities.heightAlignment,
            bitrateRange = videoCapabilities.bitrateRange.let { it.lower..it.upper },
            encoderSizeRates = sizes.flatMap { size ->
                FPS_CANDIDATES.filter { fps ->
                    videoCapabilities.areSizeAndRateSupported(size.width, size.height, fps.toDouble())
                }.map { fps -> SizeRate(size.width, size.height, fps) }
            }.toSet(),
        )
    }

    /**
     * The AVC encoder every capability query and start uses, by name: a
     * surface-capable regular encoder. Querying and starting the same codec
     * keeps the capabilities honest.
     */
    fun selectAvcSurfaceEncoder(): MediaCodecInfo? =
        MediaCodecList(MediaCodecList.REGULAR_CODECS).codecInfos.firstOrNull { info ->
            info.isEncoder &&
                info.supportedTypes.contains(MediaFormat.MIMETYPE_VIDEO_AVC) &&
                info.getCapabilitiesForType(MediaFormat.MIMETYPE_VIDEO_AVC)
                    .colorFormats
                    .contains(MediaCodecInfo.CodecCapabilities.COLOR_FormatSurface)
        }
}
