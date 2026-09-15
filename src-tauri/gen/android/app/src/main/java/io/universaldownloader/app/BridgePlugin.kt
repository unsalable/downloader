package io.universaldownloader.app

import android.Manifest
import android.app.Activity
import android.app.DownloadManager
import android.content.ActivityNotFoundException
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Color
import android.media.MediaScannerConnection
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.OpenableColumns
import android.webkit.MimeTypeMap
import android.webkit.WebView
import androidx.activity.result.ActivityResult
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import androidx.core.content.FileProvider
import androidx.core.view.WindowCompat
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File

@InvokeArg
class PathArgs {
    lateinit var path: String
}

@InvokeArg
class BackgroundWorkArgs {
    var active: Boolean = false
    var downloads: Int = 0
    var conversions: Int = 0
    var language: String = "en"
}

@InvokeArg
class SystemBarsArgs {
    var dark: Boolean = true
}

/**
 * The Android half of `src-tauri/src/android.rs`: everything the Rust core
 * needs that only the OS can answer or do.
 */
@TauriPlugin
class BridgePlugin(private val activity: Activity) : Plugin(activity) {
    private var webView: WebView? = null

    /** A link shared from another app, held until the interface asks for it. */
    @Volatile
    private var sharedText: String? = null

    private var askedForNotifications = false

    override fun load(webView: WebView) {
        this.webView = webView
        receive(activity.intent)
        requestLegacyStorage()
    }

    override fun onNewIntent(intent: Intent) {
        if (receive(intent)) {
            // The page asks for the text itself; this only tells it to look.
            webView?.post {
                webView?.evaluateJavascript(
                    "window.dispatchEvent(new Event('ud-shared-text'))",
                    null,
                )
            }
        }
    }

    private fun receive(intent: Intent?): Boolean {
        if (intent?.action != Intent.ACTION_SEND) return false
        val text = intent.getStringExtra(Intent.EXTRA_TEXT) ?: return false
        sharedText = text
        // Consumed: a configuration change re-reads the same intent.
        intent.action = null
        return true
    }

    /** Android 10 and older need the storage permission to write to Downloads. */
    private fun requestLegacyStorage() {
        if (Build.VERSION.SDK_INT > Build.VERSION_CODES.Q) return
        val permission = Manifest.permission.WRITE_EXTERNAL_STORAGE
        if (ContextCompat.checkSelfPermission(activity, permission) != PackageManager.PERMISSION_GRANTED) {
            ActivityCompat.requestPermissions(activity, arrayOf(permission), REQUEST_STORAGE)
        }
    }

    @Command
    fun environment(invoke: Invoke) {
        @Suppress("DEPRECATION")
        val downloads = Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS)
        val result = JSObject()
        result.put("nativeLibraryDir", activity.applicationInfo.nativeLibraryDir)
        result.put("filesDir", activity.filesDir.absolutePath)
        result.put("cacheDir", activity.cacheDir.absolutePath)
        result.put("downloadsDir", downloads.absolutePath)
        invoke.resolve(result)
    }

    @Command
    fun takeSharedText(invoke: Invoke) {
        val result = JSObject()
        sharedText?.let { result.put("text", it) }
        sharedText = null
        invoke.resolve(result)
    }

    @Command
    fun setBackgroundWork(invoke: Invoke) {
        val args = invoke.parseArgs(BackgroundWorkArgs::class.java)
        try {
            if (args.active) {
                requestNotifications()
                BackgroundWorkService.update(activity, args.downloads, args.conversions, args.language)
            } else {
                BackgroundWorkService.stop(activity)
            }
            invoke.resolve()
        } catch (ex: Exception) {
            // Starting a foreground service from the background is refused on
            // Android 12+. The work carries on; it is only less protected.
            invoke.reject(ex.message ?: "background work could not be updated")
        }
    }

    /** Asked for once, at the moment a notification first has something to say. */
    private fun requestNotifications() {
        if (askedForNotifications || Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) return
        askedForNotifications = true
        val permission = Manifest.permission.POST_NOTIFICATIONS
        if (ContextCompat.checkSelfPermission(activity, permission) != PackageManager.PERMISSION_GRANTED) {
            activity.runOnUiThread {
                ActivityCompat.requestPermissions(activity, arrayOf(permission), REQUEST_NOTIFICATIONS)
            }
        }
    }

    @Command
    fun setSystemBarsTheme(invoke: Invoke) {
        val args = invoke.parseArgs(SystemBarsArgs::class.java)
        activity.runOnUiThread {
            val window = activity.window
            // The page pads itself clear of the bars, so what shows behind them
            // is the window, which has to match the page's background.
            window.decorView.setBackgroundColor(Color.parseColor(if (args.dark) "#0C0B09" else "#F7F4EE"))
            val controller = WindowCompat.getInsetsController(window, window.decorView)
            controller.isAppearanceLightStatusBars = !args.dark
            controller.isAppearanceLightNavigationBars = !args.dark
        }
        invoke.resolve()
    }

    @Command
    fun openFile(invoke: Invoke) {
        val args = invoke.parseArgs(PathArgs::class.java)
        val file = File(args.path)
        if (!file.isFile) {
            invoke.reject("${file.name} is no longer on this device")
            return
        }
        try {
            val uri = FileProvider.getUriForFile(activity, "${activity.packageName}.fileprovider", file)
            val type = MimeTypeMap.getSingleton()
                .getMimeTypeFromExtension(file.extension.lowercase()) ?: "*/*"
            val intent = Intent(Intent.ACTION_VIEW)
                .setDataAndType(uri, type)
                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
            activity.startActivity(intent)
            invoke.resolve()
        } catch (ex: ActivityNotFoundException) {
            invoke.reject("No app on this device can open ${file.name}")
        } catch (ex: Exception) {
            invoke.reject(ex.message ?: "the file could not be opened")
        }
    }

    /**
     * A file written by path into shared storage is not in the media index
     * until something asks for it to be, and the gallery, music players and the
     * system file picker all read that index rather than the disk.
     */
    @Command
    fun scanFile(invoke: Invoke) {
        val args = invoke.parseArgs(PathArgs::class.java)
        val type = MimeTypeMap.getSingleton()
            .getMimeTypeFromExtension(File(args.path).extension.lowercase())
        MediaScannerConnection.scanFile(activity.applicationContext, arrayOf(args.path), arrayOf(type), null)
        invoke.resolve()
    }

    @Command
    fun openDownloads(invoke: Invoke) {
        try {
            activity.startActivity(
                Intent(DownloadManager.ACTION_VIEW_DOWNLOADS).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            )
            invoke.resolve()
        } catch (ex: Exception) {
            invoke.reject(ex.message ?: "the Downloads folder could not be opened")
        }
    }

    @Command
    fun pickMediaFiles(invoke: Invoke) {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT)
            .addCategory(Intent.CATEGORY_OPENABLE)
            .setType("*/*")
            .putExtra(Intent.EXTRA_MIME_TYPES, arrayOf("video/*", "audio/*"))
            .putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true)
        startActivityForResult(invoke, intent, "mediaFilesPicked")
    }

    @ActivityCallback
    fun mediaFilesPicked(invoke: Invoke, result: ActivityResult) {
        val data = result.data
        if (result.resultCode != Activity.RESULT_OK || data == null) {
            invoke.resolve(pathsResult(emptyList()))
            return
        }

        val uris = mutableListOf<Uri>()
        val clips = data.clipData
        if (clips != null) {
            for (index in 0 until clips.itemCount) uris.add(clips.getItemAt(index).uri)
        } else {
            data.data?.let { uris.add(it) }
        }

        // A video can be gigabytes; copying it must not hold the UI thread.
        Thread {
            try {
                val dir = importsDir()
                invoke.resolve(pathsResult(uris.mapNotNull { copyIntoCache(it, dir) }))
            } catch (ex: Exception) {
                invoke.reject(ex.message ?: "the chosen files could not be read")
            }
        }.start()
    }

    private fun pathsResult(paths: List<String>): JSObject {
        val result = JSObject()
        result.put("paths", JSArray.from(paths.toTypedArray()))
        return result
    }

    /**
     * FFmpeg needs a real path, and a picked document is only a content URI.
     * Copies older than a day are from conversions long finished.
     */
    private fun importsDir(): File {
        val dir = File(activity.cacheDir, "imports")
        dir.mkdirs()
        val cutoff = System.currentTimeMillis() - 24L * 60 * 60 * 1000
        dir.listFiles()?.filter { it.lastModified() < cutoff }?.forEach { it.delete() }
        return dir
    }

    private fun copyIntoCache(uri: Uri, dir: File): String? {
        val resolver = activity.contentResolver
        var name = "media"
        resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { cursor ->
            if (cursor.moveToFirst() && !cursor.isNull(0)) name = cursor.getString(0)
        }
        name = name.replace('/', '_').replace(' ', '_').ifBlank { "media" }

        var target = File(dir, name)
        var attempt = 2
        while (target.exists()) {
            val stem = name.substringBeforeLast('.', name)
            val ext = name.substringAfterLast('.', "")
            target = File(dir, if (ext.isEmpty()) "$stem ($attempt)" else "$stem ($attempt).$ext")
            attempt++
        }

        val input = resolver.openInputStream(uri) ?: return null
        input.use { source -> target.outputStream().use { source.copyTo(it) } }
        return target.absolutePath
    }

    companion object {
        private const val REQUEST_STORAGE = 7301
        private const val REQUEST_NOTIFICATIONS = 7302
    }
}
