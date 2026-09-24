package io.universaldownloader.app

import android.content.res.Configuration
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
    // The window opens in the phone's theme (app_background), so the bars'
    // icons follow it too: dark on a light phone, light on a dark one. The
    // page then sets both to its own theme (BridgePlugin.setSystemBarsTheme).
    val night = (resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) ==
      Configuration.UI_MODE_NIGHT_YES
    WindowCompat.getInsetsController(window, window.decorView).run {
      isAppearanceLightStatusBars = !night
      isAppearanceLightNavigationBars = !night
    }
    // Android 8 and 9 paint the navigation bar rather than showing the window
    // through it, in a scrim of their own choosing; painted the window's own
    // colour, the icons read on it (see setSystemBarsTheme).
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
    // tauri.conf.json's backgroundColor is the desktop's dark, which on a
    // light phone would show as a dark frame before the page paints, so
    // tauri.android.conf.json repeats the window without it and the WebView
    // takes the window's colour, which follows the phone's theme.
    webView.setBackgroundColor(ContextCompat.getColor(this, R.color.app_background))
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
