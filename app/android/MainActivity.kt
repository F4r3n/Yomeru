// Replaces dx's MainActivity.kt (Dioxus.toml `android_main_activity`). Same as
// dx 0.7.10's template plus FocusShield, the background bridge and the saved
// window background; keep in step when bumping dx.
package dev.dioxus.main

import android.content.Context
import android.graphics.Color
import android.graphics.drawable.ColorDrawable
import android.os.Build
import android.os.Bundle
import android.view.View
import android.view.ViewGroup
import android.webkit.JavascriptInterface
import android.webkit.WebView
import android.widget.FrameLayout
import androidx.core.graphics.ColorUtils
import androidx.core.view.WindowInsetsCompat

typealias BuildConfig = ovh.faren.yomeru.BuildConfig

/** The dark theme's `--bg` (app/shared/src/theme.rs), the default theme. */
private const val DEFAULT_BACKGROUND = 0xFF181825.toInt()

/**
 * Where the page's last background colour is kept, so the next cold start
 * paints the window in the user's theme before the page has loaded and can
 * say which one that is.
 */
private const val SHELL_PREFS = "yomeru_shell"
private const val KEY_BACKGROUND = "window_background"

/**
 * Holds the WebView and keeps window-focus losses that happen while the
 * keyboard is up from reaching it. The WebView hides the keyboard whenever
 * its window loses focus, and opening the keyboard's own "Change keyboard"
 * picker (long press on the globe key) takes window focus: the keyboard, and
 * the picker with it, closed at once. The page and the WebView keep their
 * focus throughout, so there is nothing for the WebView to react to. The
 * matching regain is held back too, so the WebView never sees the round trip.
 */
private class FocusShield(context: Context) : FrameLayout(context) {
    private var heldLoss = false

    private fun keyboardVisible(): Boolean {
        val insets = rootWindowInsets ?: return false
        return WindowInsetsCompat.toWindowInsetsCompat(insets, this)
            .isVisible(WindowInsetsCompat.Type.ime())
    }

    override fun dispatchWindowFocusChanged(hasFocus: Boolean) {
        if (!hasFocus && keyboardVisible()) {
            heldLoss = true
            return
        }
        if (hasFocus && heldLoss) {
            heldLoss = false
            return
        }
        super.dispatchWindowFocusChanged(hasFocus)
    }
}

class MainActivity : WryActivity() {
    private var shield: FocusShield? = null
    private var webView: WebView? = null

    /**
     * Paints behind the page in its own background colour. With
     * `adjustResize` the WebView changes height as the keyboard opens and
     * closes, and until the page repaints, the strip it uncovers showed the
     * default white background. The page calls this whenever its theme is
     * applied (apply_theme_to_dom in app/shared/src/app.rs).
     */
    inner class ShellBridge {
        @JavascriptInterface
        fun setBackground(css: String) {
            val color = try {
                Color.parseColor(css.trim())
            } catch (e: IllegalArgumentException) {
                return
            }
            runOnUiThread {
                applyBackground(color)
                rememberBackground(color)
            }
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        // Before the WebView exists: the window is all there is to see.
        applyBackground(savedBackground())
        super.onCreate(savedInstanceState)
    }

    private fun savedBackground(): Int =
        getSharedPreferences(SHELL_PREFS, MODE_PRIVATE).getInt(KEY_BACKGROUND, DEFAULT_BACKGROUND)

    /**
     * Keeps `color` for the next cold start. Android 13+ also lets the app
     * pick the theme of the splash screen the system draws before any app
     * code runs, remembered across launches; older versions always use the
     * manifest's (dark) AppTheme for that first frame.
     */
    private fun rememberBackground(color: Int) {
        val prefs = getSharedPreferences(SHELL_PREFS, MODE_PRIVATE)
        // The page reports its colour on every theme application; only a
        // change is worth a write.
        if (prefs.contains(KEY_BACKGROUND) && prefs.getInt(KEY_BACKGROUND, 0) == color) {
            return
        }
        prefs.edit().putInt(KEY_BACKGROUND, color).apply()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            val light = ColorUtils.calculateLuminance(color) > 0.5
            val theme = if (light) splashStyle("AppTheme.Light") else splashStyle("AppTheme")
            if (theme != 0) {
                splashScreen.setSplashScreenTheme(theme)
            }
        }
    }

    /**
     * The style's id, or 0 if it doesn't exist. Looked up by name rather than
     * through `R.style`: our styles.xml (icon/res, with AppTheme.Light) is
     * only copied over dx's by apply.sh, after `dx bundle` has compiled this
     * file, so `R.style.AppTheme_Light` doesn't exist at that point.
     */
    @android.annotation.SuppressLint("DiscouragedApi")
    private fun splashStyle(name: String): Int =
        resources.getIdentifier(name, "style", packageName)

    private fun applyBackground(color: Int) {
        window.setBackgroundDrawable(ColorDrawable(color))
        shield?.setBackgroundColor(color)
        webView?.setBackgroundColor(color)
    }

    override fun onWebViewCreate(webView: WebView) {
        this.webView = webView
        webView.addJavascriptInterface(ShellBridge(), "YomeruShell")
        applyBackground(savedBackground())
    }

    // wry calls this (through JNI) with the WebView as the whole content.
    override fun setContentView(view: View) {
        val shield = FocusShield(this)
        this.shield = shield
        shield.addView(
            view,
            FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT)
        )
        super.setContentView(shield)
        applyBackground(savedBackground())
    }
}
