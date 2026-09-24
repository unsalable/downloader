package io.universaldownloader.app

import android.os.Build
import android.os.Bundle
import android.view.View
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.core.content.ContextCompat
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    // enableEdgeToEdge picks the bars' icons by the phone's theme, dark on a
    // light phone, overriding the app theme's. The window opens dark whatever
    // the phone's theme, so they would sit dark on dark until the page sets
    // both to its own theme (BridgePlugin.setSystemBarsTheme).
    WindowCompat.getInsetsController(window, window.decorView).run {
      isAppearanceLightStatusBars = false
      isAppearanceLightNavigationBars = false
    }
    // Android 8 and 9 paint the navigation bar rather than showing the window
    // through it, in a pale scrim on a light phone; painted the window's own
    // colour, the light icons read on it (see setSystemBarsTheme).
    if (Build.VERSION.SDK_INT in Build.VERSION_CODES.O until Build.VERSION_CODES.Q) {
      @Suppress("DEPRECATION") // from Android 15, which this never reaches
      window.navigationBarColor = ContextCompat.getColor(this, R.color.app_background)
    }
    super.onCreate(savedInstanceState)
  }

  /**
   * Edge-to-edge is enforced from Android 15, which would put the page under
   * the status bar, the navigation bar and the keyboard. Padding the content
   * view by those insets keeps every control reachable; the window behind the
   * bars is painted to match the page by `BridgePlugin.setSystemBarsTheme`.
   */
  override fun onWebViewCreate(webView: WebView) {
    val content = findViewById<View>(android.R.id.content)
    ViewCompat.setOnApplyWindowInsetsListener(content) { view, insets ->
      val bars = insets.getInsets(
        WindowInsetsCompat.Type.systemBars() or
          WindowInsetsCompat.Type.displayCutout() or
          WindowInsetsCompat.Type.ime()
      )
      view.setPadding(bars.left, bars.top, bars.right, bars.bottom)
      WindowInsetsCompat.CONSUMED
    }
    ViewCompat.requestApplyInsets(content)
  }
}
