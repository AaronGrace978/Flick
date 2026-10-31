package dev.flick.radar

import java.net.InetSocketAddress
import java.nio.ByteBuffer
import java.nio.channels.SelectionKey
import java.nio.channels.Selector
import java.nio.channels.SocketChannel
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

/** Independent paired file transport. Manual sends require no microphone or IMU. */
class TransferClient(private val host: String, private val token: ByteArray,
    private val queued: () -> OutgoingAsset?, private val delivered: (OutgoingAsset) -> Unit,
    private val report: (String) -> Unit,
    private val controlPort: Int = 8089, private val dataPort: Int = 8090,
    private val finished: () -> Unit = {}) : AutoCloseable {
    private val executor = Executors.newSingleThreadExecutor()
    private val busy = AtomicBoolean(false)
    private val closed = AtomicBoolean(false)
    @Volatile private var socket: SocketChannel? = null
    private var lastId = 0L
    @Synchronized fun capture(id: Long) {
        if (closed.get() || id == lastId) return
        val asset = queued() ?: return
        if (!busy.compareAndSet(false, true)) return
        lastId = id
        submit(id, asset)
    }
    fun sendNow() {
        check(!closed.get()) { "Transfer client closed" }
        val asset = queued() ?: error("Choose a file or clipboard text first")
        if (!busy.compareAndSet(false, true)) return
        submit(null, asset)
    }
    private fun submit(capture: Long?, asset: OutgoingAsset) {
        try { executor.execute {
            try {
                val id=capture ?: PairingClient.requestTransfer(host,token,controlPort)
                report("Sending ${if(asset.kind==1) asset.name else "clipboard text"} to desktop inbox…")
                upload(id,asset)
                delivered(asset)
                report("Received by desktop and saved in inbox.")
            } catch (e: Exception) {
                if (!closed.get()) report("Transfer failed: ${e.message}. Check desktop TCP $dataPort firewall access. Payload remains queued.")
            } finally { socket=null;busy.set(false);finished() }
        }} catch (e: java.util.concurrent.RejectedExecutionException) {
            busy.set(false);if(!closed.get())throw e
        }
    }
    private fun upload(id:Long,asset:OutgoingAsset) {
        SocketChannel.open().use { channel ->
            socket=channel;channel.configureBlocking(false);channel.socket().tcpNoDelay=true
            Selector.open().use { selector ->
                channel.register(selector,SelectionKey.OP_CONNECT)
                channel.connect(InetSocketAddress(host,dataPort))
                val deadline=System.nanoTime()+10_000_000_000L
                fun await() {
                    check(!closed.get() && !Thread.currentThread().isInterrupted) { "Transfer cancelled" }
                    check(System.nanoTime()<deadline) { "Transfer timed out" }
                    selector.select(20);selector.selectedKeys().clear()
                }
                while(!channel.finishConnect())await()
                channel.keyFor(selector).interestOps(SelectionKey.OP_WRITE)
                for(buffer in arrayOf(Handoff.header(token,id,asset),ByteBuffer.wrap(asset.bytes))) {
                    while(buffer.hasRemaining()){await();channel.write(buffer)}
                }
                channel.keyFor(selector).interestOps(SelectionKey.OP_READ)
                val ack=ByteBuffer.allocate(2)
                while(ack.hasRemaining()){await();check(channel.read(ack)>=0) {"Desktop rejected the payload"}}
                check(ack.array().contentEquals(byteArrayOf(79,75))) {"Invalid receipt acknowledgement"}
            }
        }
    }
    override fun close() {
        if(!closed.compareAndSet(false,true))return
        try {socket?.close()}catch(_:java.io.IOException){}
        executor.shutdownNow()
    }
}
