package dev.nekomario.amb

import android.app.Activity
import android.app.Instrumentation
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.hardware.usb.UsbManager
import android.os.Build
import android.os.Bundle
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit

/** Exercises the platform notification boundary without pretending a USB device is attached. */
class UsbPermissionInstrumentation : Instrumentation() {
    override fun onCreate(arguments: Bundle?) {
        super.onCreate(arguments)
        start()
    }

    override fun onStart() {
        val context = targetContext
        val received = LinkedBlockingQueue<Intent>()
        val receiver = object : BroadcastReceiver() {
            override fun onReceive(context: Context, intent: Intent) {
                received.add(intent)
            }
        }
        val filter = IntentFilter(MainActivity.ACTION_USB_PERMISSION)
        if (Build.VERSION.SDK_INT >= 33) {
            context.registerReceiver(receiver, filter, Context.RECEIVER_NOT_EXPORTED)
        } else {
            @Suppress("UnspecifiedRegisterReceiverFlag")
            context.registerReceiver(receiver, filter)
        }
        try {
            val stale = MainActivity.createUsbPermissionRequest(context, "old-activity")
            val replacement = MainActivity.createUsbPermissionRequest(context, "new-activity")
            check(runCatching { stale.send() }.exceptionOrNull() is PendingIntent.CanceledException) {
                "Old Activity request survived replacement"
            }
            for (id in listOf("new-activity", "next-request")) {
                val request = if (id == "new-activity") replacement else MainActivity.createUsbPermissionRequest(context, id)
                request.send(context, 0, Intent().putExtra(UsbManager.EXTRA_PERMISSION_GRANTED, true))
                val result = checkNotNull(received.poll(5, TimeUnit.SECONDS)) { "Callback not delivered" }
                check(result.getStringExtra(MainActivity.EXTRA_USB_REQUEST_ID) == id) { "Request identity lost" }
                check(!result.hasExtra(UsbManager.EXTRA_PERMISSION_GRANTED)) { "Immutable request received fill-in extras" }
                check(runCatching { request.send() }.exceptionOrNull() is PendingIntent.CanceledException) {
                    "USB request could be replayed"
                }
            }
            finish(Activity.RESULT_OK, Bundle().apply {
                putString("stream", "PASS USB permission callback: stale request cancellation, identity, immutable extras, one-shot delivery\n")
            })
        } catch (error: Throwable) {
            finish(Activity.RESULT_CANCELED, Bundle().apply { putString("stream", "FAIL: $error\n") })
        } finally {
            context.unregisterReceiver(receiver)
        }
    }
}
