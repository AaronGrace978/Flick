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
        check(bank.shift(tone(48000, 20117.1875)) == null)
    }
    verify("Broadband noise is rejected; beacon survives moderate noise") {
        val random = Random(978)
        val noise = ShortArray(Goertzel.N) { (random.nextGaussian() * 1000).toInt().coerceIn(-32768, 32767).toShort() }
        check(Goertzel(48000).shift(noise) == null)
        val signal = tone(48000, 20042.0)
        for (i in signal.indices) signal[i] = (signal[i] + noise[i]).toShort()
        val shift = Goertzel(48000).shift(signal)
        check(shift != null && abs(shift - 42) < 0.5) { "Noisy shift=$shift" }
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
        check(runCatching { Telemetry.encode(b, Estimate(1, Float.NaN, Imu(), Imu())) }.isFailure)
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
    println("$passed core regression checks passed")
}
