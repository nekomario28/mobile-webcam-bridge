package dev.nekomario.amb

import android.content.Context
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraManager
import android.hardware.camera2.CaptureRequest
import android.media.MediaCodec
import android.os.Build
import android.util.Range
import android.util.Size

/** Camera2 capabilities used to choose an encoder-native capture mode. */
object CameraCatalog {
    data class CameraInfo(
        val id: String,
        val lensFacing: Int?,
        val physicalCameraIds: Set<String>,
        val encoderSizes: List<Size>,
        val aeFpsRanges: List<Range<Int>>,
        val focalLengthsMm: List<Float>,
        val supportsContinuousVideoAf: Boolean,
    )

    fun enumerate(context: Context): List<CameraInfo> {
        val manager = context.getSystemService(CameraManager::class.java)
        return manager.cameraIdList.map { id ->
            val c = manager.getCameraCharacteristics(id)
            val streamMap = c.get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP)
            val encoderSizes = streamMap
                ?.getOutputSizes(MediaCodec::class.java)
                ?.distinct()
                ?.sortedWith(
                    compareByDescending<Size> { it.width.toLong() * it.height }
                        .thenByDescending { it.width },
                )
                .orEmpty()

            val physicalIds = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                c.physicalCameraIds.toSet()
            } else {
                emptySet()
            }

            val afModes = c.get(CameraCharacteristics.CONTROL_AF_AVAILABLE_MODES) ?: intArrayOf()
            CameraInfo(
                id = id,
                lensFacing = c.get(CameraCharacteristics.LENS_FACING),
                physicalCameraIds = physicalIds,
                encoderSizes = encoderSizes,
                aeFpsRanges = c.get(CameraCharacteristics.CONTROL_AE_AVAILABLE_TARGET_FPS_RANGES)
                    ?.toList()
                    ?.sortedWith(compareBy<Range<Int>> { it.upper }.thenBy { it.lower })
                    .orEmpty(),
                focalLengthsMm = c.get(CameraCharacteristics.LENS_INFO_AVAILABLE_FOCAL_LENGTHS)
                    ?.toList()
                    .orEmpty(),
                supportsContinuousVideoAf = afModes.contains(
                    CaptureRequest.CONTROL_AF_MODE_CONTINUOUS_VIDEO,
                ),
            )
        }
    }

    fun chooseSize(info: CameraInfo, requestedWidth: Int, requestedHeight: Int): Size? {
        if (info.encoderSizes.isEmpty()) return null
        info.encoderSizes.firstOrNull {
            it.width == requestedWidth && it.height == requestedHeight
        }?.let { return it }

        val targetArea = requestedWidth.toLong() * requestedHeight
        val targetAspect = requestedWidth.toDouble() / requestedHeight
        return info.encoderSizes.minByOrNull { size ->
            val aspect = size.width.toDouble() / size.height
            val aspectPenalty = kotlin.math.abs(aspect - targetAspect) * 10_000_000.0
            val areaPenalty = kotlin.math.abs(size.width.toLong() * size.height - targetArea).toDouble()
            aspectPenalty + areaPenalty
        }
    }

    fun chooseFpsRange(info: CameraInfo, fps: Int): Range<Int>? {
        return info.aeFpsRanges
            .filter { fps in it.lower..it.upper }
            .minWithOrNull(
                compareBy<Range<Int>>(
                    { if (it.lower == fps && it.upper == fps) 0 else 1 },
                    { it.upper - it.lower },
                    { kotlin.math.abs(it.upper - fps) },
                ),
            )
    }
}
