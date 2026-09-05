package com.tokamak.runtime

import java.io.File

/** The tokamak runtime running in this process. */
internal class TokamakRuntime private constructor(private var handle: Long) {
    private val lock = Any()

    /** The loopback port the gateway bound. */
    val port: Int
        get() = synchronized(lock) { nativePort(requireHandle()) }

    fun restoreGateway(): Int = synchronized(lock) {
        nativeRestoreGateway(requireHandle())
    }

    fun suspend() = synchronized(lock) {
        handle.takeIf { it != 0L }?.let(::nativeSuspend)
    }

    fun resume() = synchronized(lock) {
        handle.takeIf { it != 0L }?.let(::nativeResume)
    }

    fun stop() = synchronized(lock) {
        if (handle == 0L) return
        val stopped = handle
        handle = 0
        nativeStop(stopped)
    }

    /**
     * The authority a server certificate for [host] must chain to, or null
     * when tokamak does not vouch for the host.
     */
    fun serverAuthority(host: String): ByteArray? = synchronized(lock) {
        handle.takeIf { it != 0L }?.let { nativeServerAuthority(it, host) }
    }

    /**
     * The client certificate and PKCS#8 private key, both DER, to present for
     * [host]. Null when tokamak cannot authenticate the connection.
     */
    fun clientIdentity(host: String, previousFailures: Int): Array<ByteArray>? =
        synchronized(lock) {
            handle.takeIf { it != 0L }?.let {
                nativeClientIdentity(it, host, previousFailures)
            }
        }

    private fun requireHandle(): Long {
        check(handle != 0L) { "tokamak runtime has stopped" }
        return handle
    }

    companion object {
        init {
            System.loadLibrary("tokamak")
        }

        /** Start the runtime, throwing when it cannot serve the app. */
        fun start(packagedDir: File, stateDir: File, host: String): TokamakRuntime =
            TokamakRuntime(nativeStart(packagedDir.path, stateDir.path, host))

        /** Start the runtime against a host development server. */
        fun startDevelopment(
            stateDir: File,
            host: String,
            endpoint: String,
            sessionToken: String,
        ): TokamakRuntime =
            TokamakRuntime(nativeStartDevelopment(stateDir.path, host, endpoint, sessionToken))

        @JvmStatic
        private external fun nativeStart(packagedDir: String, stateDir: String, host: String): Long

        @JvmStatic
        private external fun nativeStartDevelopment(
            stateDir: String,
            host: String,
            endpoint: String,
            sessionToken: String,
        ): Long

        @JvmStatic
        private external fun nativePort(handle: Long): Int

        @JvmStatic
        private external fun nativeRestoreGateway(handle: Long): Int

        @JvmStatic
        private external fun nativeSuspend(handle: Long)

        @JvmStatic
        private external fun nativeResume(handle: Long)

        @JvmStatic
        private external fun nativeStop(handle: Long)

        @JvmStatic
        private external fun nativeServerAuthority(handle: Long, host: String): ByteArray?

        @JvmStatic
        private external fun nativeClientIdentity(
            handle: Long,
            host: String,
            previousFailures: Int,
        ): Array<ByteArray>?
    }
}
