package io.universaldownloader.app

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
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat

/**
 * Keeps the process in the foreground while downloads or conversions run.
 *
 * It does no work of its own -- the Rust core does that -- but without it
 * Android freezes a backgrounded app within seconds, so a download would stop
 * as soon as the user switched apps or turned the screen off. It exists only
 * while there is something to protect.
 */
class BackgroundWorkService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val notification = build(this, intent)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }
        running = true
        return START_NOT_STICKY
    }

    /** Android 15 caps data-sync services at six hours a day. */
    override fun onTimeout(startId: Int, fgsType: Int) {
        stopSelf()
    }

    override fun onDestroy() {
        running = false
        super.onDestroy()
    }

    companion object {
        private const val CHANNEL_ID = "background-work"
        private const val NOTIFICATION_ID = 7310
        private const val EXTRA_DOWNLOADS = "downloads"
        private const val EXTRA_CONVERSIONS = "conversions"
        private const val EXTRA_LANGUAGE = "language"

        @Volatile
        private var running = false

        fun update(context: Context, downloads: Int, conversions: Int, language: String) {
            val intent = Intent(context, BackgroundWorkService::class.java)
                .putExtra(EXTRA_DOWNLOADS, downloads)
                .putExtra(EXTRA_CONVERSIONS, conversions)
                .putExtra(EXTRA_LANGUAGE, language)

            if (running) {
                // Already in the foreground: refresh the text in place. Asking
                // to start the service again would be refused if the app has
                // meanwhile gone to the background.
                val manager = context.getSystemService(NotificationManager::class.java)
                manager.notify(NOTIFICATION_ID, build(context, intent))
            } else {
                ContextCompat.startForegroundService(context, intent)
            }
        }

        fun stop(context: Context) {
            context.stopService(Intent(context, BackgroundWorkService::class.java))
        }

        private fun build(context: Context, intent: Intent?): Notification {
            val downloads = intent?.getIntExtra(EXTRA_DOWNLOADS, 0) ?: 0
            val conversions = intent?.getIntExtra(EXTRA_CONVERSIONS, 0) ?: 0
            val turkish = intent?.getStringExtra(EXTRA_LANGUAGE) == "tr"

            val manager = context.getSystemService(NotificationManager::class.java)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                val name = if (turkish) "Arka plan işleri" else "Background work"
                manager.createNotificationChannel(
                    NotificationChannel(CHANNEL_ID, name, NotificationManager.IMPORTANCE_LOW)
                )
            }

            val parts = mutableListOf<String>()
            if (downloads > 0) {
                parts.add(if (turkish) "$downloads indirme" else if (downloads == 1) "1 download" else "$downloads downloads")
            }
            if (conversions > 0) {
                parts.add(if (turkish) "$conversions dönüştürme" else if (conversions == 1) "1 conversion" else "$conversions conversions")
            }
            val text = if (turkish) "${parts.joinToString(", ")} sürüyor" else "${parts.joinToString(", ")} in progress"

            val open = PendingIntent.getActivity(
                context,
                0,
                Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )

            return NotificationCompat.Builder(context, CHANNEL_ID)
                .setSmallIcon(R.drawable.ic_stat_download)
                .setContentTitle("Universal Downloader")
                .setContentText(text)
                .setContentIntent(open)
                .setOngoing(true)
                .setOnlyAlertOnce(true)
                .setSilent(true)
                .setCategory(NotificationCompat.CATEGORY_PROGRESS)
                .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE)
                .build()
        }
    }
}
