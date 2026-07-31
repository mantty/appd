package com.appd.runtime

import android.app.Activity
import android.net.Uri
import android.webkit.WebView
import androidx.webkit.JavaScriptReplyProxy
import androidx.webkit.WebMessageCompat
import androidx.webkit.WebViewCompat
import org.json.JSONObject

internal class AppdPluginError(
    val errorName: String,
    message: String,
) : Exception(message) {
    companion object {
        fun notSupported(message: String) = AppdPluginError("NotSupportedError", message)
    }
}

internal typealias AppdPluginReply = (Result<Any?>) -> Unit

internal interface AppdPlugin {
    val id: String

    fun call(method: String, arguments: Any?, reply: AppdPluginReply) {
        reply(Result.failure(AppdPluginError.notSupported("$id.$method is not supported")))
    }

    fun subscribe(
        method: String,
        arguments: Any?,
        reply: AppdPluginReply,
    ): () -> Unit {
        reply(Result.failure(AppdPluginError.notSupported("$id.$method is not supported")))
        return {}
    }

    fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray,
    ) = Unit
}

internal class AppdPluginBridge(
    private val activity: Activity,
    private val host: String,
    plugins: List<AppdPlugin>,
) : WebViewCompat.WebMessageListener {
    private data class RequestKey(val session: String, val id: Int)

    private val plugins =
        buildMap {
            plugins.forEach { plugin ->
                check(put(plugin.id, plugin) == null) { "Duplicate plugin ID" }
            }
        }
    private val cancellations = mutableMapOf<RequestKey, () -> Unit>()
    private var activeSession: String? = null

    fun install(webView: WebView) {
        WebViewCompat.addWebMessageListener(
            webView,
            "__appdNative",
            setOf("https://$host"),
            this,
        )
    }

    fun close() {
        activeSession = null
        val cancelAll = cancellations.values.toList()
        cancellations.clear()
        cancelAll.forEach { it() }
    }

    fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray,
    ) {
        plugins.values.forEach {
            it.onRequestPermissionsResult(requestCode, permissions, grantResults)
        }
    }

    override fun onPostMessage(
        view: WebView,
        message: WebMessageCompat,
        sourceOrigin: Uri,
        isMainFrame: Boolean,
        replyProxy: JavaScriptReplyProxy,
    ) {
        if (
            !isMainFrame ||
                sourceOrigin.scheme != "https" ||
                sourceOrigin.host != host ||
                (sourceOrigin.port != -1 && sourceOrigin.port != 443)
        ) {
            return
        }
        val request = runCatching { JSONObject(message.data ?: return) }.getOrNull() ?: return
        val session = request.optString("session")
        if (session.isEmpty()) return
        if (request.optString("type") == "reset") {
            close()
            activeSession = session
            return
        }
        val id = request.optInt("id", -1)
        if (id < 0) return
        val key = RequestKey(session, id)
        when (request.optString("type")) {
            "cancel" -> cancellations.remove(key)?.invoke()
            "call", "subscribe" -> dispatch(request, key, replyProxy)
            else ->
                send(
                    replyProxy,
                    key,
                    Result.failure(
                        AppdPluginError.notSupported("Plugin operation is not supported"),
                    ),
                    true,
                )
        }
    }

    private fun dispatch(
        request: JSONObject,
        key: RequestKey,
        replyProxy: JavaScriptReplyProxy,
    ) {
        val plugin = plugins[request.optString("plugin")]
        val method = request.optString("method")
        if (plugin == null || method.isEmpty()) {
            send(
                replyProxy,
                key,
                Result.failure(AppdPluginError.notSupported("Plugin is not supported")),
                true,
            )
            return
        }
        val arguments = request.opt("arguments")
        val reply: AppdPluginReply = { result ->
            send(replyProxy, key, result, request.optString("type") == "call")
        }
        if (request.optString("type") == "call") {
            plugin.call(method, arguments, reply)
        } else {
            val cancellation = plugin.subscribe(method, arguments, reply)
            cancellations[key] = cancellation
        }
    }

    private fun send(
        replyProxy: JavaScriptReplyProxy,
        key: RequestKey,
        result: Result<Any?>,
        done: Boolean,
    ) {
        if (activeSession != key.session) return
        val error = result.exceptionOrNull()
        val finished = done
        if (finished) cancellations.remove(key)?.invoke()
        val response =
            JSONObject()
                .put("session", key.session)
                .put("id", key.id)
                .put("done", finished)
        if (error == null) {
            response.put("value", JSONObject.wrap(result.getOrNull()))
        } else {
            val name = (error as? AppdPluginError)?.errorName ?: "UnknownError"
            response.put(
                "error",
                JSONObject().put("name", name).put("message", error.message ?: name),
            )
        }
        activity.runOnUiThread {
            if (activeSession == key.session) replyProxy.postMessage(response.toString())
        }
    }
}
