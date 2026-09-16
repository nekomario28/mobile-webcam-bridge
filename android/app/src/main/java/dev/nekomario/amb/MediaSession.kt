package dev.nekomario.amb

interface MediaSession : AutoCloseable {
    fun attachIdrRequester(requester: (() -> Unit)?)
    fun onCodecConfig(config: H264Encoder.CodecConfig)
    fun onCameraStarted(config: CameraEncoderSession.ActiveConfig)
    fun onAccessUnit(unit: H264Encoder.AccessUnit)
    fun droppedVideoFrames(): Long
}
