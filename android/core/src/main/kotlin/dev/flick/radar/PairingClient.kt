package dev.flick.radar

import java.net.InetSocketAddress
import java.net.PortUnreachableException
import java.nio.ByteBuffer
import java.nio.channels.DatagramChannel
import java.nio.channels.SelectionKey
import java.nio.channels.Selector
import java.security.SecureRandom

/** Real desktop reachability/key check, before microphone acquisition begins. */
object PairingClient {
    fun check(host: String, code: ByteArray, port: Int = 8089, timeoutMs: Long = 3500) {
        require(timeoutMs in 100..10000)
        val nonce = SecureRandom().nextLong()
        val request = Handoff.pairRequest(code, nonce)
        var waitingForUi = false
        DatagramChannel.open().use { channel ->
            channel.configureBlocking(false)
            channel.connect(InetSocketAddress(host, port))
            Selector.open().use { selector ->
                channel.register(selector, SelectionKey.OP_READ)
                val until = System.nanoTime() + timeoutMs * 1_000_000
                var nextSend = 0L
                val response = ByteBuffer.allocate(64)
                while (System.nanoTime() < until) {
                    check(!Thread.currentThread().isInterrupted) { "Connection check cancelled" }
                    val now = System.nanoTime()
                    try {
                        if (now >= nextSend) {
                            channel.write(request.duplicate())
                            nextSend = now + 250_000_000
                        }
                        selector.select(50); selector.selectedKeys().clear()
                        repeat(4) {
                            response.clear()
                            if (channel.read(response) > 0) {
                                response.flip()
                                when (Handoff.pairReply(response, nonce)) {
                                    Handoff.PairReply.CONNECTED -> return
                                    Handoff.PairReply.REJECTED -> error("Wrong pairing code. Use pairing.key from this desktop.")
                                    Handoff.PairReply.WAITING -> waitingForUi = true
                                    null -> Unit
                                }
                            }
                        }
                    } catch (_: PortUnreachableException) {
                        error("Desktop is not listening on UDP 8089. Start flick-overlay and allow it through the firewall.")
                    }
                }
            }
        }
        if (waitingForUi) error("Desktop replied, but its visual layer is not ready. Restart flick-overlay.")
        error("Desktop did not reply. Check its LAN IPv4, same Wi-Fi, UDP 8089 firewall access, and matching updated desktop/APK versions.")
    }
}
