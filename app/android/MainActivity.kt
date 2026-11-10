// Replaces dx's MainActivity.kt (Dioxus.toml `android_main_activity`). Same as
// dx 0.7.10's template plus FocusShield and the background bridge; keep in
// step when bumping dx.
package dev.dioxus.main

import android.content.Context
import android.graphics.Color
import android.graphics.drawable.ColorDrawable
import android.view.View
import android.view.ViewGroup
import android.webkit.JavascriptInterface
import android.webkit.WebView
import android.widget.FrameLayout
import androidx.core.view.WindowInsetsCompat

typealias BuildConfig = ovh.faren.yomeru.BuildConfig

/** The dark theme's `--bg` (app/shared/src/theme.rs), the default theme. */
private const val DEFAULT_BACKGROUND = 0xFF181825.toInt()

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
            runOnUiThread { applyBackground(color) }
        }
    }

    private fun applyBackground(color: Int) {
        window.setBackgroundDrawable(ColorDrawable(color))
        shield?.setBackgroundColor(color)
        webView?.setBackgroundColor(color)
    }

    override fun onWebViewCreate(webView: WebView) {
        this.webView = webView
        webView.addJavascriptInterface(ShellBridge(), "YomeruShell")
        applyBackground(DEFAULT_BACKGROUND)
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
        applyBackground(DEFAULT_BACKGROUND)
    }
}
