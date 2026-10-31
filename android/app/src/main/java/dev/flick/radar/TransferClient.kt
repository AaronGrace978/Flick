package dev.flick.radar

import java.net.InetSocketAddress
import java.nio.ByteBuffer
import java.nio.channels.SelectionKey
import java.nio.channels.Selector
import java.nio.channels.SocketChannel
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

/** Non-blocking TCP on a dedicated worker; UDP/audio never wait for a file transfer. */
class TransferClient(private val host: String, private val token: ByteArray,
    private val queued: () -> OutgoingAsset?, private val delivered: (OutgoingAsset) -> Unit,
    private val report: (String) -> Unit) : AutoCloseable {
    private val executor = Executors.newSingleThreadExecutor()
    private val busy = AtomicBoolean(false)
    private val closed = AtomicBoolean(false)
    @Volatile private var socket: SocketChannel? = null
    private var lastId = 0L // Called only by the UDP sender thread.
    fun capture(id: Long) {
        if (closed.get() || id == lastId) return
        val asset = queued() ?: return
        if (!busy.compareAndSet(false, true)) return
        lastId = id
        executor.execute {
            try {
                SocketChannel.open().use { channel ->
                    socket = channel
                    channel.configureBlocking(false)
                    channel.socket().tcpNoDelay = true
                    Selector.open().use { selector ->
                        channel.register(selector, SelectionKey.OP_CONNECT)
                        channel.connect(InetSocketAddress(host, 8090))
                        val deadline = System.nanoTime() + 10_000_000_000L
                        fun await() {
                            check(!closed.get() && !Thread.currentThread().isInterrupted) { "Transfer cancelled" }
                            check(System.nanoTime() < deadline) { "Transfer timed out" }
                            selector.select(20); selector.selectedKeys().clear()
                        }
                        while (!channel.finishConnect()) await()
                        channel.keyFor(selector).interestOps(SelectionKey.OP_WRITE)
                        for (buffer in arrayOf(Handoff.header(token, id, asset), ByteBuffer.wrap(asset.bytes))) {
                            while (buffer.hasRemaining()) { await(); channel.write(buffer) }
                        }
                        channel.keyFor(selector).interestOps(SelectionKey.OP_READ)
                        val ack = ByteBuffer.allocate(2)
                        while (ack.hasRemaining()) { await(); check(channel.read(ack) >= 0) { "Desktop rejected the payload" } }
                        check(ack.array().contentEquals(byteArrayOf(79, 75))) { "Invalid receipt acknowledgement" }
                        delivered(asset)
                        report("Asset received by desktop; completing the visual handoff")
                    }
                }
            } catch (e: Exception) { if (!closed.get()) report("Transfer failed: ${e.message}. Payload remains queued; retry the gesture.") }
            finally { socket = null; busy.set(false) }
        }
    }
    override fun close() {
        if (!closed.compareAndSet(false, true)) return
        try { socket?.close() } catch (_: java.io.IOException) { }
        executor.shutdownNow()
    }
}
