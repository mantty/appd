package com.appd.runtime

import android.app.Activity
import android.content.pm.PackageManager
import android.os.Bundle
import android.util.Log
import android.view.ViewGroup
import android.view.WindowInsets
import android.webkit.WebView
import android.widget.FrameLayout
import androidx.webkit.WebViewFeature
import java.io.File

private const val TAG = "appd"
private const val HOST_METADATA = "appd.host"
private const val STARTING = "<!doctype html><title>Starting</title>"
private const val FAILED =
    "<!doctype html><title>appd</title><h1>App failed to start</h1><p>See logcat for details.</p>"
private const val UNSUPPORTED =
    "<!doctype html><title>appd</title><h1>Unsupported WebView</h1>" +
        "<p>This device's WebView cannot serve the app's secure origin.</p>"

/** The appd application: owns the window, WebView, and runtime lifecycle. */
class AppdActivity : Activity() {
    private lateinit var webView: WebView
    private lateinit var pluginBridge: AppdPluginBridge
    private var runtime: AppdRuntime? = null
    private var suspended = false
    private var destroyed = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        webView = WebView(this).apply {
            settings.javaScriptEnabled = true
            settings.domStorageEnabled = true
        }
        pluginBridge = AppdPluginBridge(this, appHost(), appdPlugins(this))
        if (WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)) {
            pluginBridge.install(webView)
        }
        setContentView(webViewContainer())
        if (!proxyIsSupported()) {
            show(UNSUPPORTED)
            return
        }
        show(STARTING)
        Thread(::startRuntime, "appd-startup").start()
    }

    override fun onPause() {
        super.onPause()
        suspended = true
        runtime?.suspend()
    }

    override fun onResume() {
        super.onResume()
        suspended = false
        runtime?.resume()
    }

    override fun onDestroy() {
        destroyed = true
        AppdProxy.release(this)
        pluginBridge.close()
        webView.stopLoading()
        webView.destroy()
        runtime?.stop()
        runtime = null
        super.onDestroy()
    }

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray,
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        pluginBridge.onRequestPermissionsResult(requestCode, permissions, grantResults)
    }

    private fun startRuntime() {
        val started = runCatching { AppdRuntime.start(unpackApp(), stateDir(), appHost()) }
        runOnUiThread { finishStart(started) }
    }

    private fun finishStart(started: Result<AppdRuntime>) {
        if (destroyed) {
            started.getOrNull()?.stop()
            return
        }
        val runtime = started.getOrElse { error ->
            Log.e(TAG, "appd failed to start", error)
            show(FAILED)
            return
        }
        this.runtime = runtime
        if (suspended) runtime.suspend()
        AppdProxy.acquire(this, appHost(), runtime.port)
    }

    internal fun proxyReady() {
        if (destroyed) return
        val runtime = runtime ?: return
        webView.webViewClient = AppdWebViewClient(runtime, pluginBridge)
        webView.loadUrl("https://${appHost()}/")
    }

    private fun proxyIsSupported(): Boolean =
        WebViewFeature.isFeatureSupported(WebViewFeature.PROXY_OVERRIDE) &&
            WebViewFeature.isFeatureSupported(WebViewFeature.PROXY_OVERRIDE_REVERSE_BYPASS) &&
            WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)

    private fun show(html: String) = webView.loadData(html, "text/html", "utf-8")

    private fun webViewContainer(): FrameLayout =
        FrameLayout(this).apply {
            addView(
                webView,
                FrameLayout.LayoutParams(
                    ViewGroup.LayoutParams.MATCH_PARENT,
                    ViewGroup.LayoutParams.MATCH_PARENT,
                ),
            )
            setOnApplyWindowInsetsListener { view, insets ->
                val bars = insets.getInsets(WindowInsets.Type.systemBars())
                view.setPadding(bars.left, bars.top, bars.right, bars.bottom)
                insets
            }
        }

    /** Copy the packaged app out of the APK so Bare can read it as files. */
    private fun unpackApp(): File {
        val app = File(filesDir, "appd/app")
        app.deleteRecursively()
        copyAsset("app", app)
        return app
    }

    private fun copyAsset(source: String, destination: File) {
        val entries = assets.list(source) ?: emptyArray()
        if (entries.isEmpty()) {
            destination.parentFile?.mkdirs()
            assets.open(source).use { input ->
                destination.outputStream().use(input::copyTo)
            }
            return
        }
        destination.mkdirs()
        for (entry in entries) copyAsset("$source/$entry", File(destination, entry))
    }

    private fun stateDir(): File = File(filesDir, "appd/state").apply { mkdirs() }

    private fun appHost(): String {
        val info = packageManager.getApplicationInfo(packageName, PackageManager.GET_META_DATA)
        return requireNotNull(info.metaData?.getString(HOST_METADATA)) {
            "$HOST_METADATA is required"
        }
    }

}
