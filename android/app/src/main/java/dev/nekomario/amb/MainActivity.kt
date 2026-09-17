package dev.nekomario.amb

import android.Manifest
import android.annotation.SuppressLint
import android.app.Activity
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.hardware.camera2.CameraCharacteristics
import android.hardware.usb.UsbAccessory
import android.hardware.usb.UsbManager
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.ParcelFileDescriptor
import android.view.WindowManager
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView

class MainActivity : Activity() {
    private enum class ConnectionMode { USB, LAN }

    private lateinit var usb: UsbManager
    private lateinit var status: TextView
    private var receiverRegistered = false
    private var connectionMode = ConnectionMode.USB
    private var accessoryFd: ParcelFileDescriptor? = null
    private var session: MediaSession? = null
    private var lanServer: LanServer? = null
    private var cameraBridge: CameraBridgeController? = null
    private var pendingCameraStart = false
    private var usbPermissionPending = false

    private val handler = Handler(Looper.getMainLooper())
    private val rediscoverRunnable = Runnable {
        if (connectionMode == ConnectionMode.USB && session == null && !isFinishing && !isDestroyed) {
            discoverUsb()
        }
    }

    private val permissionReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            if (intent.action != ACTION_USB_PERMISSION) return
            usbPermissionPending = false
            val accessory = intent.accessoryExtra() ?: run {
                scheduleRediscover()
                return
            }
            if (intent.getBooleanExtra(UsbManager.EXTRA_PERMISSION_GRANTED, false)) {
                openAccessory(accessory)
            } else {
                show(ui("USB accessory permission denied", "USB accessoryの権限がありません"))
                scheduleRediscover()
            }
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        usb = getSystemService(UsbManager::class.java)

        status = TextView(this).apply {
            textSize = 17f
            setPadding(32, 32, 32, 32)
            setTextIsSelectable(true)
        }
        val usbMode = Button(this).apply {
            text = ui("Connect USB", "USBで接続")
            setOnClickListener { startUsbMode() }
        }
        val lanMode = Button(this).apply {
            text = ui("Connect Wi-Fi", "Wi-Fiで接続")
            setOnClickListener { startLanMode() }
        }
        val disconnect = Button(this).apply {
            text = ui("Disconnect", "接続を停止")
            setOnClickListener {
                stopConnections()
                show(ui("Disconnected", "接続を停止しました"))
            }
        }
        val cameras = Button(this).apply {
            text = ui("Camera info", "カメラ情報")
            setOnClickListener { showCameras() }
        }
        val startCamera = Button(this).apply {
            text = ui("Start camera 720p30", "カメラ開始 720p30")
            setOnClickListener { startCamera720p() }
        }
        val stopCamera = Button(this).apply {
            text = ui("Stop camera", "カメラ停止")
            setOnClickListener {
                stopCamera()
                showConnectionStatus(ui("Camera stopped", "カメラを停止しました"))
            }
        }
        setContentView(LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            addView(status, LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT)
            addView(usbMode, LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT)
            addView(lanMode, LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT)
            addView(disconnect, LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT)
            addView(cameras, LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT)
            addView(startCamera, LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT)
            addView(stopCamera, LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT)
        })

        val filter = IntentFilter(ACTION_USB_PERMISSION)
        if (Build.VERSION.SDK_INT >= 33) {
            registerReceiver(permissionReceiver, filter, Context.RECEIVER_NOT_EXPORTED)
        } else {
            @SuppressLint("UnspecifiedRegisterReceiverFlag")
            registerReceiver(permissionReceiver, filter)
        }
        receiverRegistered = true
        startUsbMode()
    }

    override fun onResume() {
        super.onResume()
        if (::usb.isInitialized && connectionMode == ConnectionMode.USB && session == null) discoverUsb()
    }

    override fun onPause() {
        handler.removeCallbacks(rediscoverRunnable)
        super.onPause()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        if (connectionMode == ConnectionMode.USB && intent.action == UsbManager.ACTION_USB_ACCESSORY_ATTACHED) {
            usbPermissionPending = false
            discoverUsb()
        }
    }

    private fun startUsbMode() {
        handler.removeCallbacks(rediscoverRunnable)
        stopCamera()
        lanServer?.close()
        lanServer = null
        closeSession()
        connectionMode = ConnectionMode.USB
        show(ui("USB: waiting for Android Open Accessory…", "USB: Android Open Accessoryを待っています…"))
        discoverUsb()
    }

    private fun startLanMode() {
        handler.removeCallbacks(rediscoverRunnable)
        stopCamera()
        closeSession()
        connectionMode = ConnectionMode.LAN
        lanServer?.close()

        lateinit var server: LanServer
        server = LanServer(
            onSession = { lanSession ->
                runOnUiThread {
                    if (lanServer !== server) {
                        lanSession?.close()
                        return@runOnUiThread
                    }
                    if (lanSession == null) {
                        if (session is LanSession) {
                            stopCamera()
                            session = null
                        }
                        showLanWaiting(server)
                    } else {
                        closeSession()
                        session = lanSession
                        show(
                            ui("Wi-Fi: PC connected\n", "Wi-Fi: PC接続済み\n") +
                                server.endpoints().joinToString() +
                                ui("\nPress Start camera", "\nカメラ開始を押してください"),
                        )
                    }
                }
            },
            onStatus = { runOnUiThread { showLanWaiting(server) } },
            onError = { runOnUiThread { showLanWaiting(server, it) } },
        )
        lanServer = server
        runCatching { server.start() }
            .onFailure {
                lanServer = null
                show(ui("Could not start Wi-Fi server: ${it.message}", "Wi-Fi待受を開始できません: ${it.message}"))
            }
    }

    private fun showLanWaiting(server: LanServer, detail: String? = null) {
        val endpoints = server.endpoints().ifEmpty {
            listOf(ui("Check the phone Wi-Fi IPv4 address", "Wi-Fi IPv4を確認してください"))
        }
        show(
            buildString {
                append(ui("Wi-Fi: connect from the PC\n", "Wi-Fi: PCから接続してください\n"))
                append(endpoints.joinToString("\n"))
                if (!detail.isNullOrBlank()) append("\n").append(detail)
            },
        )
    }

    private fun discoverUsb() {
        handler.removeCallbacks(rediscoverRunnable)
        if (connectionMode != ConnectionMode.USB || session != null) return

        val accessory = usb.accessoryList?.firstOrNull()
        if (accessory == null) {
            usbPermissionPending = false
            show(ui("USB: waiting for AOA accessory…", "USB: AOA accessoryを待っています…"))
            scheduleRediscover()
            return
        }
        if (usb.hasPermission(accessory)) {
            usbPermissionPending = false
            openAccessory(accessory)
            return
        }
        if (!usbPermissionPending) {
            val permissionIntent = PendingIntent.getBroadcast(
                this,
                0,
                Intent(ACTION_USB_PERMISSION).setPackage(packageName),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
            usbPermissionPending = true
            usb.requestPermission(accessory, permissionIntent)
        }
        show(ui("USB: waiting for accessory permission…", "USB: accessory permissionを待っています…"))
    }

    private fun scheduleRediscover() {
        handler.removeCallbacks(rediscoverRunnable)
        if (connectionMode == ConnectionMode.USB && !isFinishing && !isDestroyed && session == null) {
            handler.postDelayed(rediscoverRunnable, REDISCOVER_DELAY_MS)
        }
    }

    private fun openAccessory(accessory: UsbAccessory) {
        if (connectionMode != ConnectionMode.USB) return
        closeSession()
        val pfd = usb.openAccessory(accessory)
        if (pfd == null) {
            show(ui("Could not open USB accessory. Retrying…", "USB accessoryを開けません。再試行します…"))
            scheduleRediscover()
            return
        }
        accessoryFd = pfd

        lateinit var active: AccessorySession
        active = AccessorySession(
            pfd = pfd,
            onProgress = { count ->
                runOnUiThread {
                    if (session === active) {
                        show(ui("USB connected · control=$count · dropped=${active.droppedVideoFrames()}\nPress Start camera",
                               "USB接続済み · control=$count · dropped=${active.droppedVideoFrames()}\nカメラ開始を押してください"))
                    }
                }
            },
            onError = { error ->
                runOnUiThread {
                    if (session === active) {
                        closeSession()
                        show(ui("USB disconnected: $error\nWaiting to reconnect…",
                               "USB切断: $error\n再接続を待っています…"))
                        scheduleRediscover()
                    }
                }
            },
        )
        session = active
        active.start()
        show(ui("USB connected · ${accessory.manufacturer} ${accessory.model}\nPress Start camera",
               "USB接続済み · ${accessory.manufacturer} ${accessory.model}\nカメラ開始を押してください"))
    }

    private fun showCameras() {
        val text = runCatching {
            CameraCatalog.enumerate(this).joinToString("\n\n") { camera ->
                val facing = when (camera.lensFacing) {
                    CameraCharacteristics.LENS_FACING_BACK -> ui("back", "背面")
                    CameraCharacteristics.LENS_FACING_FRONT -> ui("front", "前面")
                    CameraCharacteristics.LENS_FACING_EXTERNAL -> ui("external", "外部")
                    else -> ui("unknown", "不明")
                }
                val sizes = camera.encoderSizes.take(8).joinToString { "${it.width}x${it.height}" }
                val fps = camera.aeFpsRanges.joinToString { "${it.lower}-${it.upper}" }
                ui("id=${camera.id} facing=$facing\nMediaCodec: $sizes\nFPS: $fps",
                   "カメラID=${camera.id} 向き=$facing\nMediaCodec: $sizes\nFPS: $fps")
            }
        }.getOrElse { ui("Camera query failed: ${it.message}", "カメラ情報の取得に失敗しました: ${it.message}") }
        show(text.ifBlank { ui("No Camera2 device found", "Camera2 deviceがありません") })
    }

    private fun startCamera720p() {
        val activeSession = session ?: run {
            showConnectionStatus(ui("Connect to the PC by USB or Wi-Fi first", "先にUSBまたはWi-FiでPCへ接続してください"))
            return
        }
        if (checkSelfPermission(Manifest.permission.CAMERA) != PackageManager.PERMISSION_GRANTED) {
            pendingCameraStart = true
            requestPermissions(arrayOf(Manifest.permission.CAMERA), CAMERA_PERMISSION_REQUEST)
            return
        }
        pendingCameraStart = false
        stopCamera()

        val cameras = runCatching { CameraCatalog.enumerate(this) }.getOrElse {
            show(ui("Camera enumeration failed: ${it.message}", "カメラ一覧の取得に失敗しました: ${it.message}"))
            return
        }
        val camera = cameras.firstOrNull { it.lensFacing == CameraCharacteristics.LENS_FACING_BACK }
            ?: cameras.firstOrNull()
        if (camera == null) {
            show(ui("No Camera2 camera found", "Camera2 cameraがありません"))
            return
        }

        lateinit var bridge: CameraBridgeController
        bridge = CameraBridgeController(
            context = this,
            session = activeSession,
            onStatus = { message -> runOnUiThread { showConnectionStatus(ui("Streaming\n$message", "配信中\n$message")) } },
            onError = { message ->
                runOnUiThread {
                    if (cameraBridge === bridge) {
                        stopCamera()
                        showConnectionStatus(ui("Camera error: $message", "カメラエラー: $message"))
                    }
                }
            },
        )
        cameraBridge = bridge
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        runCatching {
            bridge.start(
                CameraEncoderSession.Request(
                    cameraId = camera.id,
                    width = 1280,
                    height = 720,
                    fps = 30,
                    bitrate = 6_000_000,
                ),
            )
        }.onFailure {
            stopCamera()
            showConnectionStatus(ui("Camera start failed: ${it.message}", "カメラ開始に失敗しました: ${it.message}"))
        }
    }

    private fun showConnectionStatus(message: String) {
        val transport = when (connectionMode) {
            ConnectionMode.USB -> "USB"
            ConnectionMode.LAN -> "Wi-Fi"
        }
        show("$transport · $message")
    }

    private fun stopCamera() {
        cameraBridge?.close()
        cameraBridge = null
        window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
    }

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray,
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode != CAMERA_PERMISSION_REQUEST) return
        val granted = grantResults.firstOrNull() == PackageManager.PERMISSION_GRANTED
        if (granted && pendingCameraStart) startCamera720p()
        else {
            pendingCameraStart = false
            showConnectionStatus(ui("Camera permission denied", "カメラ権限が必要です"))
        }
    }

    private fun closeSession() {
        stopCamera()
        val current = session
        session = null
        runCatching { current?.close() }
        runCatching { accessoryFd?.close() }
        accessoryFd = null
    }

    private fun stopConnections() {
        handler.removeCallbacks(rediscoverRunnable)
        closeSession()
        lanServer?.close()
        lanServer = null
    }

    private fun show(message: String) {
        status.text = message
    }

    private fun ui(english: String, japanese: String): String =
        if (resources.configuration.locales[0].language == "ja") japanese else english

    override fun onDestroy() {
        stopConnections()
        if (receiverRegistered) unregisterReceiver(permissionReceiver)
        super.onDestroy()
    }

    @Suppress("DEPRECATION")
    private fun Intent.accessoryExtra(): UsbAccessory? =
        if (Build.VERSION.SDK_INT >= 33) {
            getParcelableExtra(UsbManager.EXTRA_ACCESSORY, UsbAccessory::class.java)
        } else {
            getParcelableExtra(UsbManager.EXTRA_ACCESSORY)
        }

    companion object {
        private const val ACTION_USB_PERMISSION = "dev.nekomario.amb.USB_PERMISSION"
        private const val CAMERA_PERMISSION_REQUEST = 1001
        private const val REDISCOVER_DELAY_MS = 1000L
    }
}
