package com.sam.tenjee_vault

import android.os.Bundle
import android.view.View
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.activity.OnBackPressedCallback
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
  }

  override fun onWebViewCreate(webView: WebView) {
    super.onWebViewCreate(webView)
    // The WebView does not expose Android's gesture bar through CSS safe-area
    // insets. Resize its container for bars, cutouts and the keyboard instead.
    val content = findViewById<View>(android.R.id.content)
    ViewCompat.setOnApplyWindowInsetsListener(content) { view, insets ->
      val safe = insets.getInsets(
        WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout()
      )
      val keyboard = insets.getInsets(WindowInsetsCompat.Type.ime())
      view.setPadding(safe.left, safe.top, safe.right, maxOf(safe.bottom, keyboard.bottom))
      val keyboardVisible = insets.isVisible(WindowInsetsCompat.Type.ime())
      webView.evaluateJavascript(
        "document.documentElement.dataset.nativeKeyboard='$keyboardVisible';window.dispatchEvent(new Event('resize'));",
        null
      )
      WindowInsetsCompat.CONSUMED
    }
    ViewCompat.requestApplyInsets(content)
    onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
      override fun handleOnBackPressed() {
        // The frontend flushes pending note saves before changing history or
        // exiting. Returning false covers startup before its handler is ready.
        webView.evaluateJavascript(
          "!window.dispatchEvent(new Event('tenjee-mobile-back', {cancelable:true}))"
        ) { handled ->
          if (handled != "true") moveTaskToBack(true)
        }
      }
    })
  }
}
