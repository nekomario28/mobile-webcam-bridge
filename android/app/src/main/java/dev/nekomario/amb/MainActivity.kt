package dev.nekomario.amb

import android.Manifest
import android.annotation.SuppressLint
import android.app.Activity
import android.app.AlertDialog
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
    /// A session whose reader refused to stop. Until it is gone, opening another one would put
    /// two readers on one endpoint. The record keeps the *session object's* liveness question,
    /// so it survives the field below being cleared (independent review, ID-002/WR2: the first
    /// guard held for exactly one iteration because `closeSession` nulls its own field before
    /// it answers).
    private val readerHandover = ReaderHandover()
    // Read from the capture-teardown thread as well as the UI thread.
    @Volatile private var session: MediaSession? = null
    private var authorizationOwner: PhoneAuthorization? = null
    private lateinit var approvalUi: ClientApprovalUi
    private var lanServer: LanServer? = null
    private var capabilityRevision = 0
    @Volatile private var advertisedCapabilities: Wire.Capabilities? = null
    private var deviceCaptureModes: List<CaptureModes.CaptureMode> = emptyList()
    private var captureModes: List<CaptureModes.CaptureMode> = emptyList()
    private var qualityController: PhoneQualityController? = null
    private val qualityDeadlineRunnable = object : Runnable {
        override fun run() {
            qualityController?.tick()
            if (qualityController?.pending == true) handler.postDelayed(this, 250)
        }
    }
    private var lanIdentity: SecureLanIdentity? = null
    private lateinit var pairAction: Button
    private lateinit var rotateAction: Button
    private lateinit var storageAction: Button
    private lateinit var pairInfo: TextView
    // The producer teardown runs off the UI thread, so installing and detaching
    // the bridge is serialised: exactly one path closes a given bridge.
    private val cameraLock = Any()
    private var installedBridge: CameraBridgeController? = null
    private var cameraBridge: CameraBridgeController?
        get() = synchronized(cameraLock) { installedBridge }
        set(value) { synchronized(cameraLock) { installedBridge = value } }
    // Authorization loss must stop the producer off the UI thread; the owner is
    // a production class so the dispatcher cannot silently become the main looper.
    private val captureCancellation = CaptureCancellation {
        synchronized(cameraLock) { installedBridge.also { installedBridge = null } }
    }
    private lateinit var cameraButton: Button
    private lateinit var details: TextView
    /** Phone address and the how-to; shown only while Details is open. */
    private lateinit var lanHelp: TextView
    /** Hands the Wi-Fi pairing details to the PC over the USB cable. */
    private lateinit var sendPairingAction: Button
    private var lanHelpText = ""
    /** Whether the pairing key is revealed; only meaningful while Details is open. */
    private var pairingKeyVisible = false
    /** How many PCs were approved at the last update, to notice the first one. */
    private var approvedCount = 0
    private var cameraStartRequested = false
    /** Authorization storage is unusable until the user deliberately resets it. */
    private var storageFailed = false
    /** Why storage was last unusable, kept so the user (and a harness) can read it. */
    private var storageFailureReason: String? = null
    /** The storage action's first press only arms the second. */
    private var confirmResetStorage = false
    private var cameraPermissionPending = false
    private var notificationPermissionPending = false
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
        // Every other service lookup in this app treats the return as nullable;
        // this one assigned it straight to a non-null field, which the platform
        // stub rejects and which would NPE on first use if it were ever null.
        usb = requireNotNull(getSystemService(UsbManager::class.java)) {
            "USB service unavailable"
        }

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
        lanHelp = TextView(this).apply {
            id = R.id.lan_help
            textSize = 12f
            setTextIsSelectable(true)
            visibility = android.view.View.GONE
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
                applySecondaryVisibility()
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
                    startCamera()
                }
            }
        }
        pairInfo = TextView(this).apply {
            textSize = 12f
            setTextIsSelectable(true)
            visibility = View.GONE
        }
        pairAction = Button(this).apply {
            text = ui("Show pairing key", "接続キーを表示")
            setAllCaps(false)
            // The pairing controls belong to the Wi-Fi flow; startUsbMode hides them.
            visibility = View.GONE
            setOnClickListener {
                setPairingKeyVisible(!pairingKeyVisible)
            }
        }
        storageAction = Button(this).apply {
            id = R.id.storage_action
            setAllCaps(false)
            visibility = View.GONE
            text = ui("Reset authorization data…", "承認データを初期化…")
            setOnClickListener {
                if (!confirmResetStorage) {
                    // One control, two presses: the label states what the second
                    // press does and nothing happens until then.
                    confirmResetStorage = true
                    text = ui(
                        "Press again to erase the record and its key",
                        "もう一度押すと記録と鍵を消去します"
                    )
                    return@setOnClickListener
                }
                confirmResetStorage = false
                text = ui("Reset authorization data…", "承認データを初期化…")
                // `clear`, not a store instance: constructing one reads the record,
                // which is precisely what cannot be read in this state.
                val outcome = runCatching { ClientStore.clear(this@MainActivity) }
                val failure = outcome.exceptionOrNull()
                // Logged under the tag the conformance workflow already collects,
                // so a failure on the device names its reason instead of leaving
                // the harness to report that an effect did not arrive.
                if (failure == null) {
                    android.util.Log.i("SecureWifiGate", "reset authorization data: ok")
                } else {
                    android.util.Log.e("SecureWifiGate", "reset authorization data failed: $failure", failure)
                }
                if (failure == null && createAuthorizationOwner()) {
                    show(ui(
                        "Authorization data was reset. Every PC must be approved again.",
                        "承認データを初期化しました。すべてのPCで再許可が必要です。"
                    ))
                    if (connectionMode == ConnectionMode.LAN) startLanMode() else startUsbMode()
                } else {
                    val reason = failure?.message ?: storageFailureReason ?: "storage is still unusable"
                    show(ui("Reset failed: $reason", "初期化に失敗しました: $reason"))
                }
            }
        }
        sendPairingAction = Button(this).apply {
            text = ui("Send Wi-Fi pairing to PC", "Wi-Fi設定をPCへ送る")
            setAllCaps(false)
            visibility = View.GONE
            setOnClickListener {
                val usbSession = session
                if (usbSession == null) {
                    show(ui("Connect the PC over USB first", "先にUSBでPCへ接続してください"))
                    return@setOnClickListener
                }
                runCatching {
                    val identity = lanIdentity ?: SecureLanIdentity(this@MainActivity).also {
                        lanIdentity = it
                    }
                    Wire.encodePairingInfo(
                        LanServer.localIpv4Addresses(),
                        identity.fingerprint(),
                        identity.pairingCode(),
                    )
                }.onSuccess { payload ->
                    usbSession.sendPairingInfo(payload)
                    show(ui(
                        "Wi-Fi pairing sent to the PC. Nothing to type there.",
                        "Wi-Fi設定をPCへ送りました。PC側の入力は不要です。"
                    ))
                }.onFailure {
                    show(ui(
                        "Could not send the pairing details: ${it.message}",
                        "Wi-Fi設定を送れませんでした: ${it.message}"
                    ))
                }
            }
        }
        rotateAction = Button(this).apply {
            text = ui("Rotate Wi-Fi access key", "Wi-Fi接続キーを変更")
            setAllCaps(false)
            visibility = View.GONE
            setOnClickListener {
                AlertDialog.Builder(this@MainActivity)
                    .setTitle(ui("Rotate the Wi-Fi access key?", "Wi-Fi接続キーを変更しますか？"))
                    .setMessage(ui(
                        "Wi-Fi clients will disconnect and need the new key. Per-PC grants remain; remove them under Authorized PCs.",
                        "Wi-Fi接続を切断し、新しいキーが必要になります。PCの許可は残ります。取り消すには「許可したPC」を開いてください。"
                    ))
                    .setNegativeButton(ui("Cancel", "キャンセル"), null)
                    .setPositiveButton(ui("Rotate key", "キーを変更")) { _, _ ->
                        runCatching {
                            lanIdentity?.rotate()
                            lanServer?.close()
                            lanServer = null
                            setPairingKeyVisible(false)
                            startLanMode()
                        }.onFailure { show("Wi-Fi key rotation failed: " + it.message) }
                    }
                    .show()
            }
        }
        approvalUi = ClientApprovalUi(this, { authorizationOwner }, { session }, {
            stopConnections()
            show(ui("Approval action failed. Connection stopped; changes may not have been saved.",
                "承認操作に失敗しました。接続を停止しました。変更は保存されていない可能性があります。"))
        }, ::ui)
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
                addView(approvalUi.root, LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT)
                addView(cameraButton, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, dp(56)).apply { topMargin = dp(12) })
                addView(cameras, LinearLayout.LayoutParams.MATCH_PARENT, dp(48))
                // Everything below appears only while Details is open: the address and
                // the how-to are instructions, the pairing key is the fallback for a PC
                // that cannot take it over the cable, and both destructive controls
                // (rotate, erase) belong behind a disclosure. Each must be added here:
                // a control that is built but never added is a button the user can
                // never reach.
                addView(lanHelp, LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT)
                addView(pairAction, LinearLayout.LayoutParams.MATCH_PARENT, dp(48))
                addView(pairInfo, LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT)
                addView(sendPairingAction, LinearLayout.LayoutParams.MATCH_PARENT, dp(48))
                addView(rotateAction, LinearLayout.LayoutParams.MATCH_PARENT, dp(48))
                addView(storageAction, LinearLayout.LayoutParams.MATCH_PARENT, dp(48))
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
        if (!createAuthorizationOwner()) return
        if (getSharedPreferences("settings", MODE_PRIVATE).getString("connectionMode", "USB") == "LAN") {
            startLanMode()
        } else {
            startUsbMode()
        }
    }

    override fun onResume() {
        super.onResume()
        foreground = true
        // The notification's Stop action arrives as an intent; consume it once.
        if (consumeStopIntent(intent)) return
        refreshQualityCapabilities()
        if (cameraStartRequested) startCamera()
        if (::usb.isInitialized && connectionMode == ConnectionMode.USB && session == null) discoverUsb()
    }

    override fun onPause() {
        foreground = false
        // Streaming deliberately continues while the phone is used for something
        // else — that is what a webcam is for — and the ongoing notification from
        // `SessionService` is what tells the user it continues. What stops here is
        // only what must not survive in the task switcher.
        handler.removeCallbacks(rediscoverRunnable)
        // Never leave the secret visible in the task switcher or on return.
        pairingKeyVisible = false
        pairInfo.text = ""
        pairAction.text = ui("Show pairing key", "接続キーを表示")
        window.clearFlags(WindowManager.LayoutParams.FLAG_SECURE)
        applySecondaryVisibility()
        super.onPause()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        // The notification's Stop must work whether or not this Activity is in front:
        // with `launchMode="singleTop"` a second delivery takes this path, so handling
        // it only in onResume left the button dead in exactly the case the notification
        // exists for (the user looking at the app while streaming).
        if (consumeStopIntent(intent)) return
        if (connectionMode == ConnectionMode.USB && intent.action == UsbManager.ACTION_USB_ACCESSORY_ATTACHED) {
            clearUsbPermissionRequest()
            usbPermissionDeclined = false
            discoverUsb()
        }
    }

    /**
     * Stop capture if, and only if, this intent carries the process's own stop token.
     *
     * The Activity is exported, so `ACTION_STOP_SESSION` alone proves nothing: any
     * installed app can start an explicit component with any action, and without this
     * check it could end a live stream. A forged intent is ignored rather than consumed,
     * so the next genuine one is still honoured.
     */
    private fun consumeStopIntent(intent: Intent?): Boolean {
        if (!StopAction.accepts(intent?.action, intent?.getStringExtra(EXTRA_STOP_TOKEN))) return false
        setIntent(Intent(intent).setAction(null))
        stopCamera()
        // Measured on a physical phone: the camera really stopped (0 frames, the
        // foreground service gone) while the status still read "streaming".
        showConnectionStatus(ui("Camera stopped", "カメラを停止しました"))
        return true
    }

    /** Only the UI owner queries/revises capabilities. Reader threads see an
     * immutable snapshot, never camera or Activity state. */
    private fun refreshQualityCapabilities() {
        val previousModes = captureModes
        deviceCaptureModes = runCatching {
            val cameras = CameraCatalog.enumerate(this)
            val camera = cameras.firstOrNull { it.lensFacing == CameraCharacteristics.LENS_FACING_BACK } ?: cameras.firstOrNull()
            camera?.let { CaptureModes.collectCapabilities(this, it.id) }?.let(CaptureModes::supported).orEmpty()
        }.getOrDefault(emptyList())
        captureModes = EditionPolicy.active.filter(deviceCaptureModes)
        val permission = when {
            checkSelfPermission(Manifest.permission.CAMERA) != PackageManager.PERMISSION_GRANTED -> Wire.PERMISSION_PENDING
            captureModes.isEmpty() || captureModes.size > Wire.MAX_QUALITY_MODES -> Wire.PERMISSION_UNAVAILABLE
            else -> Wire.PERMISSION_READY
        }
        val ordered = if (captureModes.isEmpty()) emptyList() else {
            val preferred = CaptureModes.autoPreferred(captureModes)
            listOf(preferred) + captureModes.filter { it != preferred }
        }
        val modes = if (permission == Wire.PERMISSION_READY) ordered.map {
            Wire.QualityMode(it.width, it.height, it.targetFps, it.bitrate)
        } else emptyList()
        if (previousModes != captureModes || advertisedCapabilities?.let { it.permission != permission || it.modes != modes } != false) {
            capabilityRevision = capabilityRevision % 65535 + 1
        }
        advertisedCapabilities = Wire.Capabilities(1, permission, capabilityRevision, qualityController?.epoch ?: 0, modes)
        session?.updateCapabilities()
    }

    private fun bindQualitySession(active: MediaSession) {
        qualityController = PhoneQualityController(
            session = active,
            revision = { capabilityRevision },
            modes = { if (advertisedCapabilities?.permission == Wire.PERMISSION_READY) captureModes else emptyList() },
            permissionReady = { checkSelfPermission(Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED },
            foreground = { foreground },
            streaming = { cameraBridge != null },
            stopCapture = { clearStartRequest -> stopCamera(clearStartRequest = clearStartRequest, cancelQuality = false) },
            startCapture = { mode -> cameraStartRequested = true; startCamera(mode) },
            startingEpoch = advertisedCapabilities?.streamEpoch ?: 0,
            deviceModes = { deviceCaptureModes },
        )
    }

    private fun onQualityCommand(origin: MediaSession, command: Wire.HostCommand) {
        handler.post {
            if (session !== origin || origin.clientAuthorization?.isAuthorized() != true) return@post
            qualityController?.handle(command)
            handler.removeCallbacks(qualityDeadlineRunnable)
            if (qualityController?.pending == true) handler.postDelayed(qualityDeadlineRunnable, 250)
        }
    }

    /**
     * Authorization ended: stop the camera/encoder without the UI thread.
     *
     * The producer teardown can wait on the encoder callback thread, so it must
     * run neither on the authorization monitor nor on the main thread. UI text
     * and control state stay on the main thread in [onAuthorizationChanged].
     */
    private fun onCaptureCancelled(origin: MediaSession) {
        // The owner is re-evaluated on the teardown thread, so a session that has
        // already been replaced is never torn down by a stale cancellation.
        captureCancellation.cancel { session === origin }
    }

    private fun onAuthorizationChanged(origin: MediaSession) {
        handler.post {
            if (session !== origin || isFinishing || isDestroyed) return@post
            if (origin.clientAuthorization?.isAuthorized() == true) {
                show(ui("PC authorized\nPress Start camera", "PCを許可しました\n「カメラ開始」を押してください"))
            } else {
                stopCamera()
                show(ui("PC authorization required", "PCの接続許可が必要です"))
            }
        }
    }

    /**
     * The one place an owner is created: startup, and after a deliberate reset.
     * Returns false when storage is unusable, which stops connections without
     * touching the stored data.
     */
    private fun createAuthorizationOwner(): Boolean {
        authorizationOwner = try {
            PhoneAuthorization(ClientStore(this).repository, onStorageFailure = {
                handler.post { authorizationStorageFailed() }
            })
        } catch (error: Exception) {
            // Keep the cause: "storage is still unusable" without it is what left
            // two earlier attempts at the recovery action undiagnosable.
            storageFailureReason = error.message ?: error.toString()
            android.util.Log.e("SecureWifiGate", "authorization storage unavailable: $error", error)
            authorizationStorageFailed()
            return false
        }
        storageFailureReason = null
        storageFailed = false
        updateControls()
        return true
    }

    private fun disarmStorageAction() {
        if (!confirmResetStorage) return
        confirmResetStorage = false
        storageAction.text = ui("Reset authorization data…", "承認データを初期化…")
    }

    private fun authorizationStorageFailed() {
        disarmStorageAction()
        storageFailed = true
        stopConnections()
        val previous = authorizationOwner
        authorizationOwner = null
        previous?.close()
        show(ui("Authorization storage unavailable. Access stopped; no data was reset.",
            "承認データを読み書きできません。接続を停止しました。データの初期化はしていません。"))
    }

    private fun startUsbMode() {
        if (authorizationOwner == null) return
        if (connectionMode == ConnectionMode.USB && (session != null || usbPermissionRequest != null || cameraStartRequested)) return
        handler.removeCallbacks(rediscoverRunnable)
        stopCamera()
        lanServer?.close()
        lanServer = null
        lanIdentity = null
        setPairingKeyVisible(false)
        closeSession()
        connectionMode = ConnectionMode.USB
        usbPermissionDeclined = false
        saveConnectionMode()
        discoverUsb()
    }

    private fun startLanMode() {
        val authorization = authorizationOwner ?: return
        if (connectionMode == ConnectionMode.LAN && lanServer != null) return
        handler.removeCallbacks(rediscoverRunnable)
        stopCamera()
        closeSession()
        connectionMode = ConnectionMode.LAN
        clearUsbPermissionRequest()
        saveConnectionMode()
        // Nothing is auto-revealed: the PC takes the pairing itself over the cable,
        // and the key behind Details is the fallback for a PC that cannot.
        setPairingKeyVisible(false)
        lanServer?.close()
        refreshQualityCapabilities()
        val identity = lanIdentity ?: SecureLanIdentity(this).also { lanIdentity = it }

        lateinit var server: LanServer
        server = LanServer(
            identity = identity,
            authorization = authorization,
            onAuthorization = ::onAuthorizationChanged,
            onCaptureCancelled = ::onCaptureCancelled,
            onSession = { lanSession, connected ->
                handler.post {
                    if (lanServer !== server) {
                        if (connected) lanSession.close()
                        return@post
                    }
                    if (!connected) {
                        // Only the currently displayed session may clear the UI.
                        if (session === lanSession) {
                            stopCamera()
                            session = null
                            showLanWaiting()
                        }
                    } else if (lanSession.isActive()) {
                        // Ignore a late connect event after that client disconnected.
                        closeSession()
                        session = lanSession
                        bindQualitySession(lanSession)
                        show(ui("PC connected\nWaiting for authorization", "PC接続済み\n接続許可を確認しています"))
                    }
                }
            },
            onStatus = {
                handler.post { if (lanServer === server && session == null) showLanWaiting() }
            },
            onError = { error ->
                handler.post { if (lanServer === server && session == null) showLanWaiting(error) }
            },
            capabilities = { advertisedCapabilities },
            onQuality = ::onQualityCommand,
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
        // The status says what is happening; the address and the how-to are
        // procedure, and procedure belongs behind Details.
        lanHelpText = buildString {
            append(ui("Phone IP\n", "スマホのIPアドレス\n"))
            append(addresses.joinToString("\n"))
            append(ui(
                "\nTLS 1.3 is required. The pairing key above is what the PC needs.",
                "\nTLS 1.3必須。USBケーブルならPCが自動で受け取ります。手動なら「接続キーを表示」で指紋とキーを確認できます。"
            ))
            if (!detail.isNullOrBlank()) append("\n").append(detail)
        }
        lanHelp.text = lanHelpText
        applySecondaryVisibility()
        show(ui("Wi-Fi · waiting for the PC", "Wi-Fi · PCの接続を待っています"))
    }

    private fun discoverUsb() {
        handler.removeCallbacks(rediscoverRunnable)
        if (authorizationOwner == null || connectionMode != ConnectionMode.USB || session != null) return

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
        // The system remembers the grant for this app and this accessory when the user
        // ticks the dialog's "always allow" option, so this prompt is once per install
        // rather than once per cable — measured by the user on the XQ-GE44 (2026-10-24).
        // Saying so costs one line and saves the tap the other way.
        show(ui(
            "Allow USB access — tick \"always allow\" to skip this next time",
            "Androidの確認画面でUSBの利用を許可してください（「常に許可」にチェックすると次回以降は出ません）"
        ))
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
        val authorization = authorizationOwner ?: return
        // Every physical connection is a fresh consent boundary. A prior Start
        // intent is never inherited, even after the old session became null.
        //
        // And the previous session's reader must be *gone* before a new one opens: on USB
        // those two readers share one endpoint, so a surviving one consumes this
        // connection's challenge and the host sees frames that belong to the old session.
        // Refuse while a reader from an earlier session is still alive on this endpoint, and
        // refuse on the object itself rather than on a field that was already cleared.
        if (readerHandover.refuseWhileReading()) {
            show(ui(
                "The previous USB session is still reading. Unplug and replug the cable.",
                "前のUSBセッションがまだ読み取り中です。ケーブルを挿し直してください。"
            ))
            scheduleRediscover()
            return
        }
        readerHandover.clear()
        if (!closeSession()) {
            show(ui(
                "The previous USB session did not stop. Unplug and replug the cable.",
                "前のUSBセッションが停止しませんでした。ケーブルを挿し直してください。"
            ))
            scheduleRediscover()
            return
        }
        val pfd = usb.openAccessory(accessory)
        if (pfd == null) {
            show(ui("Could not open USB accessory. Retrying…", "USB accessoryを開けません。再試行します…"))
            scheduleRediscover()
            return
        }
        accessoryFd = pfd

        refreshQualityCapabilities()
        lateinit var active: AccessorySession
        active = AccessorySession(
            pfd = pfd,
            authorization = authorization,
            onAuthorization = ::onAuthorizationChanged,
            onCaptureCancelled = ::onCaptureCancelled,
            capabilities = { advertisedCapabilities },
            onQuality = ::onQualityCommand,
            onProgress = { count ->
                handler.post {
                    if (session === active) {
                        details.text = "USB · control=$count · dropped=${active.droppedVideoFrames()}"
                    }
                }
            },
            onError = { error ->
                handler.post {
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
        bindQualitySession(active)
        active.start()
        show(ui("USB attached\nWaiting for PC authorization", "USB接続済み\nPCの接続許可を確認しています"))
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

    private fun startCamera(requestedMode: CaptureModes.CaptureMode? = qualityController?.stagedMode) {
        if (!cameraStartRequested || cameraBridge != null || !foreground) return
        if (session?.clientAuthorization?.isAuthorized() != true) {
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
        qualityController?.captureRequested()
        handler.removeCallbacks(qualityDeadlineRunnable)
        if (qualityController?.pending == true) handler.postDelayed(qualityDeadlineRunnable, 250)

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

        // Slice 01: resolve the capture mode from the device capabilities.
        // Auto for initial capture; staged/committed requests must match exactly.
        val mode = when (
            val resolution = runCatching {
                CaptureModes.collectCapabilities(this, camera.id)?.let { caps ->
                    CaptureModes.resolve(caps, CaptureModes.CapturePreference(requestedMode?.width, requestedMode?.height, requestedMode?.targetFps))
                }
            }.getOrElse {
                CaptureModes.Resolution.Rejected(it.message ?: it.javaClass.simpleName)
            }
        ) {
            is CaptureModes.Resolution.Resolved -> resolution.mode
            is CaptureModes.Resolution.Rejected -> {
                stopCamera()
                show(ui("No usable camera mode: ${resolution.reason}", "利用できるカメラモードがありません: ${resolution.reason}"))
                return
            }
            null -> {
                stopCamera()
                show(ui("No usable camera mode", "利用できるカメラモードがありません"))
                return
            }
        }

        // Keep streaming while the phone is used for something else: the service
        // holds the process in the foreground so Android does not stop camera
        // access. Measured on a physical phone: without it, pressing Home closed
        // the session and the consumer read 0 frames.
        requestNotificationPermissionIfNeeded()
        SessionService.start(this)
        lateinit var bridge: CameraBridgeController
        bridge = CameraBridgeController(
            context = this,
            session = activeSession,
            onStatus = { message ->
                handler.post {
                    if (cameraBridge === bridge) {
                        details.text = message
                        // Say who is receiving the video; the name is the PC's own
                        // claim, so it is marked as such rather than presented as an
                        // identity (the verified id is in Authorized PCs).
                        val label = activeSession.clientAuthorization?.authorizedLabel()
                        showConnectionStatus(if (label != null) {
                            ui("Streaming to $label (self-declared)", "${label} へ配信中（申告名）")
                        } else {
                            ui("Camera running", "カメラ配信中")
                        })
                    }
                }
            },
            onStarted = { config ->
                handler.post {
                    if (cameraBridge === bridge && session === activeSession) {
                        qualityController?.captureStarted(config.size.width, config.size.height, config.fps)
                        advertisedCapabilities = advertisedCapabilities?.copy(streamEpoch = qualityController?.epoch ?: 0)
                    }
                }
            },
            onError = { message ->
                handler.post {
                    if (cameraBridge === bridge) {
                        qualityController?.captureFailed()
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
                    mode = mode,
                ),
            )
        }.onFailure {
            qualityController?.captureFailed()
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
        // An armed confirmation only survives while the state it was armed in
        // does. Any other control change disarms it, so a single press can never
        // erase the data after an interruption the user did not see.
        if (!storageFailed) disarmStorageAction()
        for ((button, mode) in listOf(usbMode to ConnectionMode.USB, lanMode to ConnectionMode.LAN)) {
            button.isEnabled = authorizationOwner != null
            val selected = connectionMode == mode
            button.isSelected = selected
            button.backgroundTintList = ColorStateList.valueOf(getColor(if (selected) R.color.app_accent else R.color.app_inactive))
            button.setTextColor(if (selected) Color.WHITE else getColor(R.color.app_text))
        }
        applySecondaryVisibility()
        if (connectionMode != ConnectionMode.LAN) {
            setPairingKeyVisible(false)
        }
        // The pairing key exists to get the first PC approved. Once one is, leaving
        // it on screen is clutter and a secret with no purpose; the button still
        // brings it back for a second PC.
        val approved = runCatching { authorizationOwner?.clients()?.size ?: 0 }.getOrDefault(0)
        if (approved > 0 && approvedCount == 0 && pairingKeyVisible) {
            setPairingKeyVisible(false)
        }
        approvedCount = approved
        cameraButton.isEnabled = session?.clientAuthorization?.isAuthorized() == true || cameraBridge != null || cameraStartRequested
        if (::approvalUi.isInitialized) approvalUi.refresh()
        cameraButton.backgroundTintList = ColorStateList.valueOf(getColor(if (cameraButton.isEnabled) R.color.app_accent else R.color.app_inactive))
        cameraButton.setTextColor(if (cameraButton.isEnabled) Color.WHITE else getColor(R.color.app_muted))
        cameraButton.text = when {
            cameraBridge != null -> ui("Stop camera", "カメラ停止")
            cameraStartRequested -> ui("Cancel", "キャンセル")
            else -> ui("Start camera", "カメラ開始")
        }
    }

    /**
     * Ask for the notification permission the foreground session needs to be
     * visible. Android hides a foreground service's notification on API 33+
     * without it, which would keep the camera live while telling the user
     * nothing — measured on a physical phone: the service ran
     * (`isForeground=true types=0x40`) and no notification appeared.
     */
    private fun requestNotificationPermissionIfNeeded() {
        if (Build.VERSION.SDK_INT < 33) return
        if (checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED) return
        if (notificationPermissionPending) return
        notificationPermissionPending = true
        requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), NOTIFICATION_PERMISSION_REQUEST)
    }

    /**
     * Show or hide the pairing key.
     *
     * Extracted from the button so the app can reveal it without a tap: while no PC
     * is approved the key is the next thing the user needs, and one more press to
     * see it buys nothing. Hiding it still clears the text and the secure flag.
     */
    private fun setPairingKeyVisible(visible: Boolean) {
        if (!visible) {
            pairingKeyVisible = false
            pairInfo.text = ""
            window.clearFlags(WindowManager.LayoutParams.FLAG_SECURE)
            pairAction.text = ui("Show pairing key", "接続キーを表示")
            applySecondaryVisibility()
            return
        }
        val identity = lanIdentity ?: return
        runCatching {
            pairInfo.text = ui(
                "On a USB cable the PC takes these itself: Details → Send Wi-Fi pairing to PC.\n\n" +
                    "For a PC that cannot — enter these on it:\nPhone certificate SHA-256:\n" +
                    identity.fingerprint() + "\n\n128-bit access key:\n" + identity.pairingCode() +
                    "\n\nKeep the key private.",
                "USBケーブルならPCが自動で受け取ります: 「詳細」→「Wi-Fi設定をPCへ送る」。\n\n" +
                    "それが使えないPCには以下を入力してください:\nスマホ証明書SHA-256:\n" +
                    identity.fingerprint() + "\n\n128ビット接続キー:\n" + identity.pairingCode() +
                    "\n\n接続キーは秘密にしてください。"
            )
            // Block screenshots and Android's recent-app snapshot while the bearer
            // key is on screen.
            window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
            pairingKeyVisible = true
            pairAction.text = ui("Hide pairing key", "接続キーを隠す")
            applySecondaryVisibility()
        }.onFailure { show("Wi-Fi identity error: " + it.message) }
    }

    /**
     * Secondary and destructive controls live behind Details: rotating the key
     * disconnects every Wi-Fi client, and the phone's address plus the how-to are
     * instructions rather than state.
     */
    private fun applySecondaryVisibility() {
        val open = details.visibility == View.VISIBLE
        lanHelp.visibility = if (open && lanHelpText.isNotEmpty()) View.VISIBLE else View.GONE
        pairAction.visibility =
            if (open && connectionMode == ConnectionMode.LAN) View.VISIBLE else View.GONE
        pairInfo.visibility = if (open && pairingKeyVisible) View.VISIBLE else View.GONE
        // A failed read is not procedure: its only remedy must not be hidden behind a
        // disclosure the failure itself would prevent the user from finding.
        storageAction.visibility = if (open || storageFailed) View.VISIBLE else View.GONE
        rotateAction.visibility =
            if (open && connectionMode == ConnectionMode.LAN) View.VISIBLE else View.GONE
        // Sending pairing details needs the cable and a live session to send them on.
        // Only for an authorized PC: that is what the hand-off is for, and an unauthorized write
        // is refused by the host's authorization exchange (independent review, ID-003).
        sendPairingAction.visibility =
            if (open && connectionMode == ConnectionMode.USB &&
                session?.clientAuthorization?.isAuthorized() == true
            ) View.VISIBLE else View.GONE
    }

    private fun saveConnectionMode() {
        getSharedPreferences("settings", MODE_PRIVATE).edit()
            .putString("connectionMode", connectionMode.name).apply()
    }

    private fun stopCamera(clearStartRequest: Boolean = true, cancelQuality: Boolean = true) {
        if (clearStartRequest) cameraStartRequested = false
        // The camera is what the foreground service exists for; nothing is
        // streaming once it stops.
        SessionService.stop(this)
        val current = synchronized(cameraLock) { installedBridge.also { installedBridge = null } }
        current?.close()
        if (cancelQuality) qualityController?.cancel()
        window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        updateControls()
    }

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray,
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode == NOTIFICATION_PERMISSION_REQUEST) {
            // Nothing depends on the answer: capture continues either way, and the
            // notification appears when the user allows it (or in the system's
            // foreground-service list when they do not).
            notificationPermissionPending = false
            return
        }
        if (requestCode != CAMERA_PERMISSION_REQUEST) return
        cameraPermissionPending = false
        refreshQualityCapabilities()
        if (!cameraStartRequested) return
        val granted = grantResults.firstOrNull() == PackageManager.PERMISSION_GRANTED
        if (granted) {
            // The capability list changed from permission-pending to ready.
            startCamera()
        }
        else {
            stopCamera()
            showConnectionStatus(ui("Camera permission denied", "カメラ権限が必要です"))
        }
    }

    private fun closeSession(clearStartRequest: Boolean = true): Boolean {
        val current = session
        session = null
        qualityController = null
        handler.removeCallbacks(qualityDeadlineRunnable)
        stopCamera(clearStartRequest)
        // A USB session's reader is a blocked read on the accessory stream: closing the
        // session is not the same as that read having returned. The answer decides whether
        // a new session may open, because two readers on one endpoint take each other's
        // frames (see AccessorySession.closeAndAwait).
        val accessory = current as? AccessorySession
        val stopped = accessory?.closeAndAwait(1500) ?: true
        // Remember which session it was: the next attempt must ask *that object* whether its
        // reader is still there, since the field above no longer holds it.
        if (accessory != null) readerHandover.recordClose(stopped, accessory::readerAlive)
        runCatching { current?.close() }
        runCatching { accessoryFd?.close() }
        accessoryFd = null
        return stopped
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
        SessionService.stop(this)
        authorizationOwner?.close()
        authorizationOwner = null
        captureCancellation.shutdown()
        if (receiverRegistered) unregisterReceiver(permissionReceiver)
        super.onDestroy()
    }

    companion object {
        internal const val ACTION_USB_PERMISSION = "dev.nekomario.amb.USB_PERMISSION"

        /** From the foreground notification's Stop action. Whether the action may act is
         *  decided by [StopAction], which also owns the token, so no value is written down
         *  here for a revert to turn into a constant. */
        internal const val ACTION_STOP_SESSION = StopAction.ACTION
        internal const val EXTRA_STOP_TOKEN = StopAction.EXTRA_TOKEN

        /** The process's own token; the notification hands it back so a foreign app cannot. */
        internal fun stopToken(): String = StopAction.currentToken()
        internal const val EXTRA_USB_REQUEST_ID = "usb_permission_request_id"
        private const val CAMERA_PERMISSION_REQUEST = 1001
        private const val NOTIFICATION_PERMISSION_REQUEST = 1002
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
