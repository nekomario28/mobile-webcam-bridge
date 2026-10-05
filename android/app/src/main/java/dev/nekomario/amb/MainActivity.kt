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
import android.content.res.ColorStateList
import android.graphics.Color
import android.hardware.camera2.CameraCharacteristics
import android.hardware.usb.UsbAccessory
import android.hardware.usb.UsbManager
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.ParcelFileDescriptor
import android.view.Gravity
import android.view.View
import android.view.WindowInsets
import android.view.WindowManager
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import java.util.UUID

class MainActivity : Activity() {
    private enum class ConnectionMode { USB, LAN }

    private lateinit var usb: UsbManager
    private lateinit var status: TextView
    private lateinit var usbMode: Button
    private lateinit var lanMode: Button
    private var receiverRegistered = false
    private var connectionMode = ConnectionMode.USB
    private var accessoryFd: ParcelFileDescriptor? = null
    private var session: MediaSession? = null
    private var lanServer: LanServer? = null
    private var cameraBridge: CameraBridgeController? = null
    private lateinit var cameraButton: Button
    private lateinit var details: TextView
    private var cameraStartRequested = false
    private var cameraPermissionPending = false
    private var foreground = false
    private var usbPermissionRequest: PendingIntent? = null
    private var usbPermissionDeclined = false
    private var usbPermissionRequestId: String? = null

    private val handler = Handler(Looper.getMainLooper())
    private val rediscoverRunnable = Runnable {
        if (connectionMode == ConnectionMode.USB && session == null && !isFinishing && !isDestroyed) {
            discoverUsb()
        }
    }

    private val permissionReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            if (intent.action != ACTION_USB_PERMISSION || connectionMode != ConnectionMode.USB ||
                usbPermissionRequest == null || intent.getStringExtra(EXTRA_USB_REQUEST_ID) != usbPermissionRequestId) return
            clearUsbPermissionRequest()
            // An immutable PendingIntent cannot receive USB result extras; query the OS grant.
            val accessory = usb.accessoryList?.firstOrNull() ?: run {
                scheduleRediscover()
                return
            }
            if (usb.hasPermission(accessory)) {
                usbPermissionDeclined = false
                openAccessory(accessory)
            } else {
                usbPermissionDeclined = true
                showUsbPermissionDeclined()
                scheduleRediscover()
            }
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        usb = getSystemService(UsbManager::class.java)

        status = TextView(this).apply {
            textSize = 17f
            id = R.id.connection_status
            setTextColor(getColor(R.color.app_text))
            setPadding(0, 0, 0, dp(16))
            setTextIsSelectable(true)
        }
        usbMode = Button(this).apply {
            id = R.id.usb_mode
            text = "USB"
            setAllCaps(false)
            setOnClickListener { startUsbMode() }
        }
        lanMode = Button(this).apply {
            id = R.id.lan_mode
            text = "Wi-Fi"
            setAllCaps(false)
            setOnClickListener { startLanMode() }
        }
        details = TextView(this).apply {
            textSize = 13f
            setPadding(0, dp(8), 0, dp(8))
            setTextIsSelectable(true)
            visibility = android.view.View.GONE
        }
        val cameras = Button(this).apply {
            id = R.id.details_action
            text = ui("Details", "詳細")
            setAllCaps(false)
            backgroundTintList = ColorStateList.valueOf(Color.TRANSPARENT)
            setTextColor(getColor(R.color.app_accent))
            elevation = 0f
            stateListAnimator = null
            gravity = Gravity.START or Gravity.CENTER_VERTICAL
            setPadding(0, 0, 0, 0)
            setOnClickListener {
                if (details.visibility == android.view.View.VISIBLE) {
                    details.visibility = android.view.View.GONE
                } else {
                    details.visibility = android.view.View.VISIBLE
                    showCameras()
                }
            }
        }
        cameraButton = Button(this).apply {
            id = R.id.camera_action
            text = ui("Start camera", "カメラ開始")
            setAllCaps(false)
            setOnClickListener {
                if (cameraStartRequested) {
                    stopCamera()
                    showConnectionStatus(ui("Camera stopped", "カメラを停止しました"))
                } else {
                    cameraStartRequested = true
                    updateControls()
                    startCamera720p()
                }
            }
        }
        val root = ScrollView(this).apply {
            setBackgroundColor(getColor(R.color.app_background))
            isFillViewport = true
            addView(LinearLayout(this@MainActivity).apply {
                orientation = LinearLayout.VERTICAL
                setPadding(dp(16), dp(16), dp(16), dp(16))
                addView(status, LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT)
                addView(LinearLayout(this@MainActivity).apply {
                    orientation = LinearLayout.HORIZONTAL
                    addView(usbMode, LinearLayout.LayoutParams(0, dp(52), 1f).apply { marginEnd = dp(4) })
                    addView(lanMode, LinearLayout.LayoutParams(0, dp(52), 1f).apply { marginStart = dp(4) })
                })
                addView(cameraButton, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, dp(56)).apply { topMargin = dp(12) })
                addView(cameras, LinearLayout.LayoutParams.MATCH_PARENT, dp(48))
                addView(details, LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT)
            })
        }
        if (Build.VERSION.SDK_INT >= 30) {
            window.setDecorFitsSystemWindows(false)
        } else {
            @Suppress("DEPRECATION")
            window.decorView.systemUiVisibility = View.SYSTEM_UI_FLAG_LAYOUT_STABLE or
                View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN or View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION or
                View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR or View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR
        }
        root.setOnApplyWindowInsetsListener { view, insets ->
            if (Build.VERSION.SDK_INT >= 30) {
                val bars = insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout())
                view.setPadding(bars.left, bars.top, bars.right, bars.bottom)
            } else {
                @Suppress("DEPRECATION")
                view.setPadding(insets.systemWindowInsetLeft, insets.systemWindowInsetTop,
                    insets.systemWindowInsetRight, insets.systemWindowInsetBottom)
            }
            insets
        }
        setContentView(root)
        root.requestApplyInsets()

        val filter = IntentFilter(ACTION_USB_PERMISSION)
        if (Build.VERSION.SDK_INT >= 33) {
            registerReceiver(permissionReceiver, filter, Context.RECEIVER_NOT_EXPORTED)
        } else {
            @SuppressLint("UnspecifiedRegisterReceiverFlag")
            registerReceiver(permissionReceiver, filter)
        }
        receiverRegistered = true
        if (getSharedPreferences("settings", MODE_PRIVATE).getString("connectionMode", "USB") == "LAN") {
            startLanMode()
        } else {
            startUsbMode()
        }
    }

    override fun onResume() {
        super.onResume()
        foreground = true
        if (cameraStartRequested) startCamera720p()
        if (::usb.isInitialized && connectionMode == ConnectionMode.USB && session == null) discoverUsb()
    }

    override fun onPause() {
        foreground = false
        handler.removeCallbacks(rediscoverRunnable)
        super.onPause()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        if (connectionMode == ConnectionMode.USB && intent.action == UsbManager.ACTION_USB_ACCESSORY_ATTACHED) {
            clearUsbPermissionRequest()
            usbPermissionDeclined = false
            discoverUsb()
        }
    }

    private fun startUsbMode() {
        if (connectionMode == ConnectionMode.USB && (session != null || usbPermissionRequest != null || cameraStartRequested)) return
        handler.removeCallbacks(rediscoverRunnable)
        stopCamera()
        lanServer?.close()
        lanServer = null
        closeSession()
        connectionMode = ConnectionMode.USB
        usbPermissionDeclined = false
        saveConnectionMode()
        discoverUsb()
    }

    private fun startLanMode() {
        if (connectionMode == ConnectionMode.LAN && lanServer != null) return
        handler.removeCallbacks(rediscoverRunnable)
        stopCamera()
        closeSession()
        connectionMode = ConnectionMode.LAN
        clearUsbPermissionRequest()
        saveConnectionMode()
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
                        showLanWaiting()
                    } else {
                        closeSession(clearStartRequest = false)
                        session = lanSession
                        show(ui("PC connected\nPress Start camera", "PC接続済み\n「カメラ開始」を押してください"))
                        if (cameraStartRequested) startCamera720p()
                    }
                }
            },
            onStatus = {
                runOnUiThread { if (lanServer === server && session == null) showLanWaiting() }
            },
            onError = { error ->
                runOnUiThread { if (lanServer === server && session == null) showLanWaiting(error) }
            },
        )
        lanServer = server
        runCatching { server.start() }
            .onFailure {
                lanServer = null
                show(ui("Could not start Wi-Fi server: ${it.message}", "Wi-Fi待受を開始できません: ${it.message}"))
            }
    }

    private fun showLanWaiting(detail: String? = null) {
        val addresses = LanServer.localIpv4Addresses().ifEmpty {
            listOf(ui("Check the phone Wi-Fi IPv4 address", "Wi-Fi IPv4を確認してください"))
        }
        show(
            buildString {
                append(ui("Phone IP\n", "スマホのIPアドレス\n"))
                append(addresses.joinToString("\n"))
                append(ui("\nEnter this IP on the PC and press Connect", "\nPCにこのIPを入力し「接続」を押す"))
                if (!detail.isNullOrBlank()) append("\n").append(detail)
            },
        )
    }

    private fun discoverUsb() {
        handler.removeCallbacks(rediscoverRunnable)
        if (connectionMode != ConnectionMode.USB || session != null) return

        val accessory = usb.accessoryList?.firstOrNull()
        if (accessory == null) {
            clearUsbPermissionRequest()
            usbPermissionDeclined = false
            show(ui("USB: press Connect on the PC", "USB: PCで「接続」を押してください"))
            scheduleRediscover()
            return
        }
        if (usb.hasPermission(accessory)) {
            clearUsbPermissionRequest()
            usbPermissionDeclined = false
            openAccessory(accessory)
            return
        }
        if (usbPermissionDeclined) {
            showUsbPermissionDeclined()
            scheduleRediscover()
            return
        }
        if (usbPermissionRequest == null) {
            val requestId = UUID.randomUUID().toString()
            usbPermissionRequestId = requestId
            val permissionIntent = createUsbPermissionRequest(this, requestId)
            usbPermissionRequest = permissionIntent
            usb.requestPermission(accessory, permissionIntent)
        }
        show(ui("Allow USB access in the Android prompt", "Androidの確認画面でUSBの利用を許可してください"))
    }

    private fun showUsbPermissionDeclined() {
        show(ui("USB access declined\nTap USB to try again", "USBの利用許可がありません\nUSBを選び直すと再試行します"))
    }

    private fun clearUsbPermissionRequest() {
        usbPermissionRequest?.cancel()
        usbPermissionRequest = null
        usbPermissionRequestId = null
    }

    private fun scheduleRediscover() {
        handler.removeCallbacks(rediscoverRunnable)
        if (connectionMode == ConnectionMode.USB && !isFinishing && !isDestroyed && session == null) {
            handler.postDelayed(rediscoverRunnable, REDISCOVER_DELAY_MS)
        }
    }

    private fun openAccessory(accessory: UsbAccessory) {
        if (connectionMode != ConnectionMode.USB) return
        closeSession(clearStartRequest = false)
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
                        details.text = "USB · control=$count · dropped=${active.droppedVideoFrames()}"
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
        if (cameraStartRequested) startCamera720p()
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
        details.text = text.ifBlank { ui("No Camera2 device found", "Camera2 deviceがありません") }
    }

    private fun startCamera720p() {
        if (!cameraStartRequested || cameraBridge != null || !foreground) return
        if (session == null) {
            cameraStartRequested = false
            showConnectionStatus(ui("Connect the PC, then press Start camera", "PCを接続してからカメラ開始を押してください"))
            return
        }
        if (checkSelfPermission(Manifest.permission.CAMERA) != PackageManager.PERMISSION_GRANTED) {
            if (!cameraPermissionPending) {
                cameraPermissionPending = true
                requestPermissions(arrayOf(Manifest.permission.CAMERA), CAMERA_PERMISSION_REQUEST)
            }
            return
        }
        val activeSession = session ?: return

        val cameras = runCatching { CameraCatalog.enumerate(this) }.getOrElse {
            stopCamera()
            show(ui("Camera enumeration failed: ${it.message}", "カメラ一覧の取得に失敗しました: ${it.message}"))
            return
        }
        val camera = cameras.firstOrNull { it.lensFacing == CameraCharacteristics.LENS_FACING_BACK }
            ?: cameras.firstOrNull()
        if (camera == null) {
            stopCamera()
            show(ui("No Camera2 camera found", "Camera2 cameraがありません"))
            return
        }

        lateinit var bridge: CameraBridgeController
        bridge = CameraBridgeController(
            context = this,
            session = activeSession,
            onStatus = { message ->
                runOnUiThread {
                    if (cameraBridge === bridge) {
                        details.text = message
                        showConnectionStatus(ui("Camera running", "カメラ配信中"))
                    }
                }
            },
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
        updateControls()
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

    private fun updateControls() {
        for ((button, mode) in listOf(usbMode to ConnectionMode.USB, lanMode to ConnectionMode.LAN)) {
            val selected = connectionMode == mode
            button.isSelected = selected
            button.backgroundTintList = ColorStateList.valueOf(getColor(if (selected) R.color.app_accent else R.color.app_inactive))
            button.setTextColor(if (selected) Color.WHITE else getColor(R.color.app_text))
        }
        cameraButton.isEnabled = session != null || cameraStartRequested
        cameraButton.backgroundTintList = ColorStateList.valueOf(getColor(if (cameraButton.isEnabled) R.color.app_accent else R.color.app_inactive))
        cameraButton.setTextColor(if (cameraButton.isEnabled) Color.WHITE else getColor(R.color.app_muted))
        cameraButton.text = when {
            cameraBridge != null -> ui("Stop camera", "カメラ停止")
            cameraStartRequested -> ui("Cancel", "キャンセル")
            else -> ui("Start camera", "カメラ開始")
        }
    }

    private fun saveConnectionMode() {
        getSharedPreferences("settings", MODE_PRIVATE).edit()
            .putString("connectionMode", connectionMode.name).apply()
    }

    private fun stopCamera(clearStartRequest: Boolean = true) {
        if (clearStartRequest) cameraStartRequested = false
        cameraBridge?.close()
        cameraBridge = null
        window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        updateControls()
    }

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray,
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode != CAMERA_PERMISSION_REQUEST) return
        cameraPermissionPending = false
        if (!cameraStartRequested) return
        val granted = grantResults.firstOrNull() == PackageManager.PERMISSION_GRANTED
        if (granted) startCamera720p()
        else {
            stopCamera()
            showConnectionStatus(ui("Camera permission denied", "カメラ権限が必要です"))
        }
    }

    private fun closeSession(clearStartRequest: Boolean = true) {
        val current = session
        session = null
        stopCamera(clearStartRequest)
        runCatching { current?.close() }
        runCatching { accessoryFd?.close() }
        accessoryFd = null
    }

    private fun stopConnections() {
        handler.removeCallbacks(rediscoverRunnable)
        clearUsbPermissionRequest()
        closeSession()
        lanServer?.close()
        lanServer = null
    }

    private fun show(message: String) {
        status.text = message
        updateControls()
    }

    private fun dp(value: Int): Int = (value * resources.displayMetrics.density + 0.5f).toInt()

    private fun ui(english: String, japanese: String): String =
        if (resources.configuration.locales[0].language == "ja") japanese else english

    override fun onDestroy() {
        stopConnections()
        if (receiverRegistered) unregisterReceiver(permissionReceiver)
        super.onDestroy()
    }

    companion object {
        internal const val ACTION_USB_PERMISSION = "dev.nekomario.amb.USB_PERMISSION"
        internal const val EXTRA_USB_REQUEST_ID = "usb_permission_request_id"
        private const val CAMERA_PERMISSION_REQUEST = 1001
        private const val REDISCOVER_DELAY_MS = 1000L

        internal fun createUsbPermissionRequest(context: Context, requestId: String): PendingIntent =
            PendingIntent.getBroadcast(
                context,
                0,
                Intent(ACTION_USB_PERMISSION).setPackage(context.packageName)
                    .putExtra(EXTRA_USB_REQUEST_ID, requestId),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_ONE_SHOT or PendingIntent.FLAG_CANCEL_CURRENT,
            )
    }
}
