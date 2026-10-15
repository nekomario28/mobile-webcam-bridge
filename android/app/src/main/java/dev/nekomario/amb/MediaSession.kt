package dev.nekomario.amb

interface MediaSession : AutoCloseable {
    /** Absent on a test sink, never an authorization grant. */
    val clientAuthorization: PhoneAuthorization.Session? get() = null
    fun attachIdrRequester(requester: (() -> Unit)?)
    fun beginCapture(): Long
    fun stopVideo()
    fun onCodecConfig(config: H264Encoder.CodecConfig, generation: Long)
    fun onVideoConfig(width: Int, height: Int, fps: Int, bitrate: Int, generation: Long)
    fun onAccessUnit(unit: H264Encoder.AccessUnit, generation: Long)
    fun sendQualityResult(result: Wire.QualityResult)
    fun pauseQuality(epoch: Wire.Epoch)
    fun droppedVideoFrames(): Long
    /** Re-advertise the quality capabilities after the list changes. */
    fun updateCapabilities()
}
