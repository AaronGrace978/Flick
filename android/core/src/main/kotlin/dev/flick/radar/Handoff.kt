package dev.flick.radar

import java.nio.ByteBuffer
import java.nio.ByteOrder

/** Explicitly chosen single-use payload. Nothing reads storage/clipboard implicitly. */
data class OutgoingAsset(val kind: Int, val name: String, val bytes: ByteArray)
object Handoff {
    const val MAX_BYTES = 16 * 1024 * 1024
    const val MAX_TEXT = 256 * 1024
    fun token(hex: String): ByteArray {
        require(hex.matches(Regex("[0-9a-fA-F]{64}"))) { "Pairing code must contain 64 hexadecimal characters" }
        return ByteArray(32) { hex.substring(it * 2, it * 2 + 2).toInt(16).toByte() }
    }
    fun header(code: ByteArray, id: Long, asset: OutgoingAsset): ByteBuffer {
        require(code.size == 32 && id > 0 && asset.kind in 0..1)
        require(asset.bytes.size <= MAX_BYTES)
        val name = asset.name.toByteArray(Charsets.UTF_8)
        require(name.size <= 100)
        if (asset.kind == 0) require(name.isEmpty() && asset.bytes.size <= MAX_TEXT)
        else require(asset.name.matches(Regex("[a-zA-Z0-9_.-]{1,100}")) && !asset.name.endsWith('.'))
        return ByteBuffer.allocate(59 + name.size).order(ByteOrder.BIG_ENDIAN)
            .put("FLICK001".toByteArray(Charsets.US_ASCII)).put(code).putLong(id).put(asset.kind.toByte())
            .putShort(name.size.toShort()).putLong(asset.bytes.size.toLong()).put(name).apply { flip() }
    }
    fun capture(buffer: ByteBuffer): Long? {
        if (buffer.remaining() != 12) return null
        buffer.order(ByteOrder.BIG_ENDIAN)
        if (buffer.int != 0x464c4b31) return null
        return buffer.long.takeIf { it > 0 }
    }
}
