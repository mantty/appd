package com.appd.runtime

import android.graphics.Bitmap
import android.net.Uri
import android.net.http.SslError
import android.webkit.ClientCertRequest
import android.webkit.SslErrorHandler
import android.webkit.WebView
import android.webkit.WebViewClient
import java.io.ByteArrayInputStream
import java.security.KeyFactory
import java.security.PrivateKey
import java.security.cert.CertificateFactory
import java.security.cert.X509Certificate
import java.security.spec.PKCS8EncodedKeySpec

/** Answers the app origin's TLS challenges with the runtime's decision. */
internal class AppdWebViewClient(
    private val runtime: AppdRuntime,
    private val pluginBridge: AppdPluginBridge,
) : WebViewClient() {
    override fun onPageStarted(view: WebView, url: String, favicon: Bitmap?) {
        pluginBridge.close()
    }

    override fun onReceivedSslError(view: WebView, handler: SslErrorHandler, error: SslError) {
        val host = Uri.parse(error.url).host
        val authority = host?.let(runtime::serverAuthority)
        if (authority != null && chainsTo(error, authority)) handler.proceed() else handler.cancel()
    }

    override fun onReceivedClientCertRequest(view: WebView, request: ClientCertRequest) {
        // Android reports no failure count, so every request is a first attempt.
        val identity = runtime.clientIdentity(request.host, 0)
        if (identity == null) {
            // The gateway requires a certificate, so proceeding without one
            // ends the connection, which is the intended outcome.
            request.ignore()
            return
        }
        val certificate = certificate(identity[0])
        val privateKey = privateKey(identity[1])
        if (certificate == null || privateKey == null) {
            request.cancel()
            return
        }
        request.proceed(privateKey, arrayOf(certificate))
    }

    private fun chainsTo(error: SslError, authority: ByteArray): Boolean {
        if (error.primaryError != SslError.SSL_UNTRUSTED) return false
        val presented = error.certificate.x509Certificate ?: return false
        val issuer = certificate(authority) ?: return false
        return runCatching {
            presented.checkValidity()
            presented.verify(issuer.publicKey)
        }.isSuccess
    }

    private fun certificate(der: ByteArray): X509Certificate? =
        runCatching {
            CertificateFactory.getInstance("X.509")
                .generateCertificate(ByteArrayInputStream(der)) as X509Certificate
        }.getOrNull()

    private fun privateKey(der: ByteArray): PrivateKey? =
        runCatching {
            KeyFactory.getInstance("EC").generatePrivate(PKCS8EncodedKeySpec(der))
        }.getOrNull()
}
