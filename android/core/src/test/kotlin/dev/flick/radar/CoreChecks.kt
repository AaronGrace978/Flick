package dev.flick.radar

import java.nio.ByteBuffer
import java.util.Random
import kotlin.math.*

/** Executable regression checks, using only Kotlin/JDK standard libraries. */
fun main() {
    var passed = 0
    fun verify(name: String, body: () -> Unit) { body(); passed++; println("PASS $name") }
    fun tone(rate: Int, frequency: Double, amplitude: Double = 0.2, phase: Double = 0.37): ShortArray =
        ShortArray(Goertzel.N) { (32767 * amplitude * sin(2 * PI * frequency * it / rate + phase)).toInt().toShort() }
    verify("Doppler sign, off-bin interpolation and both capture rates") {
        for (rate in intArrayOf(44100, 48000)) for (shift in doubleArrayOf(-85.0, -30.0, -2.5, 0.0, 3.25, 37.0, 80.0)) {
            val measured = Goertzel(rate).shift(tone(rate, Goertzel.CARRIER + shift))
            check(measured != null && abs(measured - shift) < 0.25) { "rate=$rate shift=$shift measured=$measured" }
        }
    }
    verify("Silence, clipping, unrelated tones and search edges are rejected") {
        val bank = Goertzel(48000)
        check(bank.shift(ShortArray(Goertzel.N)) == null)
        check(bank.shift(ShortArray(Goertzel.N) { if (it % 2 == 0) 32767 else -32768 }) == null)
        check(bank.shift(tone(48000, 1000.0)) == null)
        val edge = Goertzel.CARRIER + (Goertzel.BINS / 2) * (48000.0 / Goertzel.N / 2)
        check(bank.shift(tone(48000, edge)) == null) { "edge $edge should be rejected" }
    }
    verify("Broadband noise is rejected; beacon survives moderate noise") {
        val random = Random(978)
        val noise = ShortArray(Goertzel.N) { (random.nextGaussian() * 1000).toInt().coerceIn(-32768, 32767).toShort() }
        check(Goertzel(48000).shift(noise) == null)
        val signal = tone(48000, 20042.0)
        for (i in signal.indices) signal[i] = (signal[i] + noise[i]).toShort()
        val shift = Goertzel(48000).shift(signal)
        check(shift != null && abs(shift - 42) < 0.5) { "Noisy shift=$shift" }
        // Loud speech-band energy must not hide a weak ultrasonic beacon.
        val masked = tone(48000, 20020.0, 0.01)
        val hum = Random(7)
        for (i in masked.indices) {
            val low = (8000 * sin(2 * PI * 120.0 * i / 48000)).toInt()
            val room = (hum.nextGaussian() * 4000).toInt()
            masked[i] = (masked[i] + low + room).toInt().coerceIn(-32768, 32767).toShort()
        }
        val heard = Goertzel(48000).shift(masked)
        check(heard != null && abs(heard - 20) < 1.0) { "Masked shift=$heard" }
    }
    verify("Competing strong ultrasonic tones are rejected") {
        val a = tone(48000, 19960.0, 0.15)
        val b = tone(48000, 20040.0, 0.15)
        val mixed = ShortArray(Goertzel.N) { (a[it] + b[it]).toShort() }
        check(Goertzel(48000).shift(mixed) == null)
    }
    verify("Window size and sample-rate contracts") {
        check(runCatching { Goertzel(16000) }.isFailure)
        check(runCatching { Goertzel(48000).shift(ShortArray(12)) }.isFailure)
    }
    verify("Stationary calibration removes bias without hiding subsequent motion") {
        val c = Calibration(); c.begin()
        repeat(60) { check(c.correct(7.0 + (it % 3 - 1) * 0.1) == null) }
        check(!c.collecting && abs(c.baselineHz - 7.0) < 1e-8)
        check(abs(c.correct(12.0)!! - 5.0) < 1e-8)
        c.begin()
        val failed = runCatching { repeat(60) { c.correct(it.toDouble()) } }.isFailure
        check(failed && !c.collecting && abs(c.baselineHz - 7.0) < 1e-8)
    }
    verify("Packet endian layout matches Rust fixture, with no padding or gyro") {
        val b = ByteBuffer.allocate(Telemetry.BYTES)
        Telemetry.encode(b, Estimate(123, .5f, Imu(123, 1f, -2f, 9.81f), Imu()))
        check(b.remaining() == 24)
        val hex = b.array().joinToString("") { "%02x".format(it.toInt() and 255) }
        check(hex == "000000000000007b3f0000003f800000c0000000411cf5c3") { hex }
        val motion = ByteBuffer.allocate(Telemetry.BYTES)
        Telemetry.encode(motion, Estimate(1, Float.NaN, Imu(1, 1f, -2f, 9.81f), Imu()))
        check(motion.getFloat(8).isNaN() && motion.getFloat(12) == 1f)
        check(runCatching { Telemetry.encode(b, Estimate(1, Float.POSITIVE_INFINITY, Imu(), Imu())) }.isFailure)
    }
    verify("Sender drops stale data and skips missed deadlines") {
        val period = Telemetry.PERIOD_NS
        check(Telemetry.nextDeadline(100, 100) == 100 + period)
        check(Telemetry.nextDeadline(100, 100 + period) == 100 + 2 * period)
        check(Telemetry.nextDeadline(100, 100 + 5 * period + 1) == 100 + 6 * period)
        val e = Estimate(200_000_000, 0f, Imu(), Imu())
        check(Telemetry.eligible(e, 0, 210_000_000))
        check(!Telemetry.eligible(e, e.time, 210_000_000))
        check(!Telemetry.eligible(e, 0, 199_999_999))
        check(!Telemetry.eligible(e, 0, 300_000_001))
        check(!Telemetry.eligible(null, 0, 210_000_000))
    }
    verify("IMU freshness and nonfinite data") {
        check(Imu(123, 1f, 2f, 3f).fresh(124))
        check(!Imu().fresh(124))
        check(!Imu(123).fresh(122))
        check(!Imu(123).fresh(100_000_124))
        check(!Imu(123, Float.NaN).fresh(124))
    }
    verify("Paired handoff header and capture control framing") {
        val code = Handoff.token("07".repeat(32))
        val header = Handoff.header(code, 42, OutgoingAsset(0, "", "hello".toByteArray()))
        check(header.remaining() == 59)
        check(header.getLong(40) == 42L && header.getLong(51) == 5L)
        check(runCatching { Handoff.token("bad") }.isFailure)
        for (name in arrayOf("song.mp3", "clip.wav", "notes.pdf", "brief.doc", "brief.docx")) {
            val header = Handoff.header(code, 42, OutgoingAsset(1, name, byteArrayOf(1)))
            check(header.remaining() == 59 + name.length) { name }
        }
        check(runCatching { Handoff.header(code, 42, OutgoingAsset(1, "../bad", byteArrayOf())) }.isFailure)
        check(runCatching { Handoff.header(code, 42, OutgoingAsset(0, "", ByteArray(Handoff.MAX_TEXT + 1))) }.isFailure)
        val ack = ByteBuffer.allocate(12).putInt(0x464c4b31).putLong(42).apply { flip() }
        check(Handoff.capture(ack) == 42L)
        check(Handoff.capture(ByteBuffer.allocate(11)) == null)
    }
    verify("Pairing probes bind nonce and reject malformed replies") {
        val request=Handoff.pairRequest(ByteArray(32) { 7 }, 42)
        check(request.remaining()==48 && request.getLong(8)==42L)
        fun reply(name:String,nonce:Long)=ByteBuffer.allocate(16).put(name.toByteArray()).putLong(nonce).apply { flip() }
        check(Handoff.pairReply(reply("FLKPOK01",42),42)==Handoff.PairReply.CONNECTED)
        check(Handoff.pairReply(reply("FLKPBAD1",42),42)==Handoff.PairReply.REJECTED)
        check(Handoff.pairReply(reply("FLKPWAIT",42),42)==Handoff.PairReply.WAITING)
        check(Handoff.pairReply(reply("FLKPOK01",41),42)==null)
        check(Handoff.pairReply(ByteBuffer.allocate(15),42)==null)
    }
    verify("Real UDP pairing succeeds without audio and reports wrong code and timeout") {
        fun server(prefix:String?, test:(Int)->Unit) {
            val socket=java.net.DatagramSocket(0,java.net.InetAddress.getByName("127.0.0.1"))
            socket.soTimeout=100
            val worker=Thread {
                while (!socket.isClosed) {
                    try {
                        val p=java.net.DatagramPacket(ByteArray(64),64);socket.receive(p)
                        if (prefix!=null && p.length==48) {
                            val response=ByteBuffer.allocate(16).put(prefix.toByteArray())
                                .put(p.data,8,8).array()
                            socket.send(java.net.DatagramPacket(response,response.size,p.socketAddress))
                        }
                    } catch (_: java.net.SocketTimeoutException) { }
                      catch (_: java.net.SocketException) { break }
                }
            }
            worker.start()
            try { test(socket.localPort) } finally { socket.close();worker.join(1000);check(!worker.isAlive) }
        }
        server("FLKPOK01") { PairingClient.check("127.0.0.1",ByteArray(32) { 7 },it,600) }
        server("FLKPBAD1") {
            val error=runCatching { PairingClient.check("127.0.0.1",ByteArray(32) { 7 },it,600) }.exceptionOrNull()
            check(error?.message?.contains("Wrong pairing code")==true)
        }
        server(null) {
            val error=runCatching { PairingClient.check("127.0.0.1",ByteArray(32) { 7 },it,200) }.exceptionOrNull()
            check(error?.message?.contains("did not reply")==true)
        }
        server("FLKPWAIT") {
            val error=runCatching { PairingClient.check("127.0.0.1",ByteArray(32) { 7 },it,200) }.exceptionOrNull()
            check(error?.message?.contains("visual layer is not ready")==true)
        }
    }
    verify("Manual send framing rejects wrong nonce and requires a positive capture ID") {
        val request=Handoff.sendRequest(ByteArray(32) {7},123)
        check(request.remaining()==48)
        check(String(request.array(),0,8,Charsets.US_ASCII)=="FLKSEND1")
        fun reply(nonce:Long,id:Long)=ByteBuffer.allocate(24).put("FLKSEND2".toByteArray()).putLong(nonce).putLong(id).apply {flip()}
        check(Handoff.sendReply(reply(123,42),123)==42L)
        check(Handoff.sendReply(reply(124,42),123)==null)
        check(Handoff.sendReply(reply(123,0),123)==null)
    }
    verify("Real manual JPG transfer reaches a disk inbox without audio or IMU") {
        val loopback=java.net.InetAddress.getByName("127.0.0.1")
        val udp=java.net.DatagramSocket(0,loopback);udp.soTimeout=2500
        val tcp=java.net.ServerSocket(0,1,loopback);tcp.soTimeout=2500
        val folder=java.nio.file.Files.createTempDirectory("flick-inbox-test")
        val data=ByteArray(32000) {(it%251).toByte()}
        val asset=OutgoingAsset(1,"photo.jpg",data)
        val failure=java.util.concurrent.atomic.AtomicReference<Throwable?>()
        val delivered=java.util.concurrent.atomic.AtomicBoolean(false)
        val done=java.util.concurrent.CountDownLatch(1)
        val worker=Thread {
            try {
                val packet=java.net.DatagramPacket(ByteArray(64),64);udp.receive(packet)
                check(packet.length==48 && String(packet.data,0,8,Charsets.US_ASCII)=="FLKSEND1")
                val reply=ByteBuffer.allocate(24).put("FLKSEND2".toByteArray()).put(packet.data,8,8).putLong(42).array()
                udp.send(java.net.DatagramPacket(reply,reply.size,packet.socketAddress))
                tcp.accept().use { socket ->
                    socket.soTimeout=2500
                    val input=java.io.DataInputStream(socket.getInputStream())
                    val header=ByteArray(59);input.readFully(header)
                    val h=ByteBuffer.wrap(header)
                    check(String(header,0,8,Charsets.US_ASCII)=="FLICK001" && h.getLong(40)==42L)
                    check(header.sliceArray(8 until 40).contentEquals(ByteArray(32) {7}))
                    val name=ByteArray(h.getShort(49).toInt());input.readFully(name)
                    check(String(name)=="photo.jpg" && h.getLong(51)==data.size.toLong())
                    val content=ByteArray(data.size);input.readFully(content)
                    java.nio.file.Files.write(folder.resolve("photo.jpg"),content)
                    socket.getOutputStream().write(byteArrayOf(79,75))
                }
            } catch(error:Throwable) {failure.set(error)}
        }
        val client=TransferClient("127.0.0.1",ByteArray(32) {7},{asset},{delivered.set(true)},{},udp.localPort,tcp.localPort,{done.countDown()})
        worker.start()
        try {
            client.sendNow();check(done.await(5,java.util.concurrent.TimeUnit.SECONDS))
            failure.get()?.let {throw it}
            check(delivered.get())
            check(java.nio.file.Files.readAllBytes(folder.resolve("photo.jpg")).contentEquals(data))
        } finally {
            client.close();udp.close();tcp.close();worker.join(3000)
            java.nio.file.Files.deleteIfExists(folder.resolve("photo.jpg"));java.nio.file.Files.deleteIfExists(folder)
        }
    }
    println("$passed core regression checks passed")
}
