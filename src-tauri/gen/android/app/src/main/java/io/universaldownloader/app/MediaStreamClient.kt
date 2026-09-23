package io.universaldownloader.app

import android.graphics.Bitmap
import android.webkit.MimeTypeMap
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.webkit.WebViewCompat
import java.io.ByteArrayInputStream
import java.io.File
import java.io.FileInputStream
import java.util.concurrent.ConcurrentHashMap

/**
 * Plays the editor's clips straight from the file.
 *
 * The editor's `<video>` reads its file from the asset protocol in byte ranges,
 * and Android's WebView cuts each range out of the answer on its own: it skips
 * to the range's first byte in the answer's stream and reads from there. The
 * protocol answers from Rust with the range alone -- at most 1000 KB of it --
 * so for any range past the start of the file the WebView skipped beyond the
 * end of what it was given, failed the request and asked again, forever, and
 * the picture never came.
 *
 * Here the answer is the whole file as a stream, which the WebView skips
 * through and reads for as long as it needs, without the file ever being held
 * in memory. Only the files Rust has let the webview read are answered this
 * way (see `allow_media_preview`); every other request goes on to the client
 * wry installed, which keeps its own scope.
 *
 * The WebView measures a stream with an `int`, so a file past 2 GB can only be
 * read up to there (see `FileStream`).
 */
class MediaStreamClient private constructor(private val inner: WebViewClient) : WebViewClient() {
    override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse? =
        serve(request) ?: inner.shouldInterceptRequest(view, request)

    override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean =
        inner.shouldOverrideUrlLoading(view, request)

    override fun onPageStarted(view: WebView, url: String, favicon: Bitmap?) =
        inner.onPageStarted(view, url, favicon)

    override fun onPageFinished(view: WebView, url: String) = inner.onPageFinished(view, url)

    override fun onReceivedError(view: WebView, request: WebResourceRequest, error: WebResourceError) =
        inner.onReceivedError(view, request, error)

    private fun serve(request: WebResourceRequest): WebResourceResponse? {
        val url = request.url
        if (url.host != ASSET_HOST || url.scheme !in WEB_SCHEMES) return null
        // HEAD as well as GET: left to the protocol, a HEAD without a range
        // would have the whole file read into memory to answer it.
        if (request.method != "GET" && request.method != "HEAD") return null
        // The path of the URL is the file's own, percent-encoded after the
        // leading slash, as `convertFileSrc` writes it; `path` has decoded it.
        val path = url.path?.removePrefix("/") ?: return null
        // Compared, and then opened, by its canonical path: `..`, a link or a
        // doubled slash on the way to an allowed file is that file, anything
        // else is not in the list, and nothing on the path can be swapped for a
        // link between the look and the open.
        val file = try {
            File(File(path).canonicalPath)
        } catch (ex: Exception) {
            return null
        }
        if (file.path !in allowed || !file.isFile) return null

        val length = file.length()
        // Thrown from here, a failure would take the app down with it: a file
        // deleted since the check is left to the protocol, which says so.
        val stream = try {
            FileStream(file, length)
        } catch (ex: Exception) {
            return null
        }
        val headers = mutableMapOf(
            "Accept-Ranges" to "bytes",
            "Cache-Control" to "no-store",
        )
        // The editor asks in CORS mode, so that Web Audio may lift the volume.
        // Only the app's own page is told yes, as the protocol tells it: any
        // other page this WebView might ever show can still play the file, as
        // any page can play any video, but not read it.
        header(request, "Origin")?.takeIf { it in PAGE_ORIGINS }?.let {
            headers["Access-Control-Allow-Origin"] = it
        }
        val type = MimeTypeMap.getSingleton().getMimeTypeFromExtension(file.extension.lowercase())
            ?: "application/octet-stream"

        // Anything but one range this reads -- none, several, or one it cannot
        // parse -- is answered with the whole file, which HTTP allows and
        // which is what the WebView reads for those too.
        val range = header(request, "Range")?.let { parseRange(it, stream.readable) }
            ?: return WebResourceResponse(type, null, 200, "OK", headers, stream)
        if (range.isEmpty()) {
            stream.close()
            headers["Content-Range"] = "bytes */$length"
            return WebResourceResponse(
                type, null, 416, "Range Not Satisfiable", headers, ByteArrayInputStream(ByteArray(0)),
            )
        }
        headers["Content-Range"] = "bytes ${range.first}-${range.last}/$length"
        return WebResourceResponse(type, null, 206, "Partial Content", headers, stream)
    }

    /**
     * The file as the WebView reads it: from its first byte, for the WebView to
     * skip through, and no further than an `int` can count.
     *
     * The WebView asks a stream how much it holds before it reads, and checks
     * the range against that. `FileInputStream` may answer from the kernel,
     * whose count is an `int` that wraps past 2 GB -- a 3 GB file can say it
     * holds nothing, a 5 GB one 1 GB -- so the count is taken from the file's
     * length here, and the stream ends where the count does, so that what the
     * WebView reads always agrees with what it was told. Up to 2 GB a file
     * plays and seeks as it would anywhere; past that, its first 2 GB can be
     * reached and a range beyond them is refused rather than misread.
     */
    private class FileStream(file: File, length: Long) : FileInputStream(file) {
        val readable = minOf(length, Int.MAX_VALUE.toLong())

        // The descriptor's own position, which every read and skip moves.
        private fun left(): Long = readable - channel.position()

        override fun available(): Int = left().coerceAtLeast(0).toInt()

        override fun read(): Int = if (left() > 0) super.read() else -1

        override fun read(b: ByteArray): Int = read(b, 0, b.size)

        override fun read(b: ByteArray, off: Int, len: Int): Int {
            if (len == 0) return 0
            val left = left()
            if (left <= 0) return -1
            return super.read(b, off, minOf(len.toLong(), left).toInt())
        }

        override fun skip(n: Long): Long = super.skip(minOf(n, left()).coerceAtLeast(0))
    }

    companion object {
        private const val ASSET_HOST = "asset.localhost"
        private val WEB_SCHEMES = setOf("http", "https")

        /**
         * The page's origin, as the protocol's answers name it. Tauri serves the
         * app from `tauri.localhost` on Android, a development build included:
         * it fetches the dev server's pages itself and serves them from there.
         */
        private val PAGE_ORIGINS = setOf("http://tauri.localhost", "https://tauri.localhost")

        /** A range no byte of the file answers, for a 416. */
        private val UNSATISFIABLE = LongRange.EMPTY

        private val allowed: MutableSet<String> = ConcurrentHashMap.newKeySet()

        /**
         * Let the webview stream one file; asked for by `android::allow_media`.
         *
         * Only ever from Rust, after `allow_media_preview` has found a file
         * there and let the protocol read it too. The page cannot reach this
         * by calling the plugin itself: none of the plugin's commands is
         * granted in `capabilities/`, and one that ever is must not be this.
         */
        fun allow(path: String) {
            allowed.add(File(path).canonicalPath)
        }

        /**
         * Put this client in front of wry's. Posted, because wry gives the
         * WebView its client after the plugins are handed the WebView.
         */
        fun install(webView: WebView) {
            webView.post {
                // Reading the client back is only possible on Android 8 or on
                // a WebView new enough to offer it; without it the picture stays
                // blank, as it would without this class. Anything else thrown
                // here is taken the same way: out of a posted task, it would
                // close the app.
                val current = try {
                    WebViewCompat.getWebViewClient(webView)
                } catch (ex: RuntimeException) {
                    return@post
                }
                if (current !is MediaStreamClient) webView.webViewClient = MediaStreamClient(current)
            }
        }

        private fun header(request: WebResourceRequest, name: String): String? =
            request.requestHeaders.entries.firstOrNull { it.key.equals(name, ignoreCase = true) }?.value

        /**
         * One range -- `bytes=a-b`, `bytes=a-` or `bytes=-n` -- as its first and
         * last byte among the first `readable`, or [UNSATISFIABLE] when none of
         * them is in it. Null for anything else: several ranges, or one that is
         * not a range at all, which HTTP says to answer as if none were asked.
         */
        private fun parseRange(value: String, readable: Long): LongRange? {
            if (!value.substringBefore('=', "").trim().equals("bytes", ignoreCase = true)) return null
            val spec = value.substringAfter('=').trim()
            val dash = spec.indexOf('-')
            if (dash < 0 || spec.contains(',')) return null
            val first = spec.substring(0, dash).trim()
            val last = spec.substring(dash + 1).trim()
            if (first.isEmpty() && last.isEmpty()) return null
            if (!first.all { it in '0'..'9' } || !last.all { it in '0'..'9' }) return null
            // Numbers too long for a `Long` are past the end of any file.
            val to = if (last.isEmpty()) null else last.toLongOrNull() ?: Long.MAX_VALUE

            if (first.isEmpty()) {
                // The last n bytes, of which there are none when n is 0.
                val suffix = to ?: return null
                if (suffix == 0L || readable == 0L) return UNSATISFIABLE
                return maxOf(readable - suffix, 0L)..readable - 1
            }
            val from = first.toLongOrNull() ?: Long.MAX_VALUE
            if (to != null && to < from) return null
            if (from >= readable) return UNSATISFIABLE
            return from..minOf(to ?: Long.MAX_VALUE, readable - 1)
        }
    }
}
