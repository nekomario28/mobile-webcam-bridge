package dev.nekomario.amb

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder

/**
 * Keeps the streaming session in the foreground so the phone can be used as a
 * webcam while its screen is showing something else.
 *
 * Measured on a physical phone (2026-10-19): pressing Home ended the session —
 * the TCP connection closed and the consumer read 0 frames — because the Activity
 * stopped the camera in `onPause` and Android stops camera access for a
 * background app. A foreground service with the camera type is the supported way
 * to keep streaming, and the ongoing notification is what tells the user that the
 * camera is still live (it carries the way to stop it).
 *
 * The service holds no capture state: it exists to keep the process in the
 * foreground. The session and the camera belong to the Activity.
 */
class SessionService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val notification = notification()
        if (Build.VERSION.SDK_INT >= 29) {
            // android:foregroundServiceType="camera" in the manifest; the camera
            // permission is already granted by the time the session starts.
            startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_CAMERA)
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }
        return START_NOT_STICKY
    }

    private fun notification(): Notification {
        val manager = getSystemService(NotificationManager::class.java)
        if (Build.VERSION.SDK_INT >= 26 && manager.getNotificationChannel(CHANNEL) == null) {
            manager.createNotificationChannel(
                NotificationChannel(CHANNEL, label("Streaming", "配信中"), NotificationManager.IMPORTANCE_LOW)
            )
        }
        // `getActivity`, not `getService`: a service that answers the action by
        // starting the Activity is blocked by the background activity-start
        // restriction on Android 10+, so the stop would silently do nothing. A
        // notification's activity PendingIntent is user-initiated and exempt, and
        // the Activity owns the camera anyway.
        val stop = PendingIntent.getActivity(
            this, 0,
            Intent(this, MainActivity::class.java)
                .setAction(MainActivity.ACTION_STOP_SESSION)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        return Notification.Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.ic_launcher)
            .setContentTitle(label("Mobile Webcam is streaming", "Mobile Webcam 配信中"))
            .setContentText(label(
                "The phone is serving its camera to the connected PC.",
                "スマホのカメラを接続中のPCに配信しています。",
            ))
            .setOngoing(true)
            .addAction(Notification.Action.Builder(null, label("Stop", "停止"), stop).build())
            .build()
    }

    private fun label(english: String, japanese: String): String =
        if (resources.configuration.locales[0].language == "ja") japanese else english

    companion object {
        private const val CHANNEL = "mobile_webcam_session"
        private const val NOTIFICATION_ID = 1

        /** Start (or keep) the foreground session; safe to call repeatedly. */
        fun start(context: Context) {
            context.startForegroundService(Intent(context, SessionService::class.java))
        }

        fun stop(context: Context) {
            context.stopService(Intent(context, SessionService::class.java))
        }
    }
}
