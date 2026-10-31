package dev.flick.radar

import android.content.Context
import android.hardware.*
import android.media.*
import android.os.*
import java.net.InetSocketAddress
import java.nio.ByteBuffer
import java.nio.channels.DatagramChannel
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

/** Foreground-only acquisition with serialized lifecycle control off the UI thread.
 * Audio and UDP workers never wait on the UI. close() is asynchronous and idempotent;
 * onClosed is delivered after audio, network and sensor resources are released.
 */
class RadarPipeline(
    context: Context,
    private val host: String,
    private val report: (String) -> Unit,
    private val onClosed: () -> Unit
) : AutoCloseable, SensorEventListener {
    private val application = context.applicationContext
    private val sensors = context.getSystemService(Context.SENSOR_SERVICE) as SensorManager
    private val audioManager = context.getSystemService(Context.AUDIO_SERVICE) as AudioManager
    private val sensorThread = HandlerThread("flick-imu")
    private val control = Executors.newSingleThreadExecutor()
    private val sender = Executors.newSingleThreadScheduledExecutor()
    private val started = AtomicBoolean(false)
    private val closed = AtomicBoolean(false)
    private val calibrate = AtomicBoolean(false)
    @Volatile private var accel = Imu()
    @Volatile private var gyro = Imu()
    @Volatile private var estimate: Estimate? = null
    private var recorder: AudioRecord? = null
    private var worker: Thread? = null
    private var channel: DatagramChannel? = null

    /** Starts native initialization on the lifecycle worker. Requires RECORD_AUDIO. */
    fun start() {
        check(!closed.get() && started.compareAndSet(false, true))
        control.execute {
            try {
                if (closed.get()) return@execute
                sensorThread.start()
                for (type in intArrayOf(Sensor.TYPE_ACCELEROMETER, Sensor.TYPE_GYROSCOPE)) {
                    val sensor = sensors.getDefaultSensor(type) ?: error("Missing required IMU sensor: $type")
                    check(sensors.registerListener(this, sensor, 5000, Handler(sensorThread.looper))) { "Cannot register IMU sensor" }
                }
                val audio = openRecorder()
                recorder = audio
                check(!closed.get()) { "Capture cancelled" }
                val udp = DatagramChannel.open()
                channel = udp
                udp.configureBlocking(false)
                // UI accepts numeric IPv4 only, so initialization needs no DNS service.
                val destination = InetSocketAddress(host, 8089)
                val buffer = ByteBuffer.allocate(Telemetry.BYTES)
                var sent = 0L
                // A one-shot schedule is advanced by elapsed time; missed periods are skipped.
                var deadline = SystemClock.elapsedRealtimeNanos()
                val send = object : Runnable {
                    override fun run() {
                        if (closed.get()) return
                        try {
                            val sample = estimate
                            val now = SystemClock.elapsedRealtimeNanos()
                            if (Telemetry.eligible(sample, sent, now)) {
                                Telemetry.encode(buffer, sample!!)
                                udp.send(buffer, destination) // would-block returns zero; drop it.
                                sent = sample.time
                            }
                            val done = SystemClock.elapsedRealtimeNanos()
                            deadline = Telemetry.nextDeadline(deadline, done)
                            if (!closed.get()) sender.schedule(this, deadline - done, TimeUnit.NANOSECONDS)
                        } catch (e: Exception) { if (!closed.get()) fail(e) }
                    }
                }
                worker = Thread({ acquire(audio) }, "flick-audio").also { it.start() }
                if (!closed.get()) {
                    sender.execute(send)
                    report("Listening at ${audio.sampleRate} Hz. Hold still and tap Calibrate.")
                }
            } catch (e: Exception) { if (!closed.get()) fail(e) }
        }
    }

    /** Requests 60 valid stationary observations. Invalid audio cancels the run. */
    fun calibrate() { calibrate.set(true) }

    /** Try each supported source/rate; some devices advertise unusable UNPROCESSED. */
    private fun openRecorder(): AudioRecord {
        if (application.checkSelfPermission(android.Manifest.permission.RECORD_AUDIO) != android.content.pm.PackageManager.PERMISSION_GRANTED)
            throw SecurityException("Microphone permission is required")
        val sources = if (audioManager.getProperty(AudioManager.PROPERTY_SUPPORT_AUDIO_SOURCE_UNPROCESSED) == "true")
            intArrayOf(MediaRecorder.AudioSource.UNPROCESSED, MediaRecorder.AudioSource.VOICE_RECOGNITION)
        else intArrayOf(MediaRecorder.AudioSource.VOICE_RECOGNITION)
        for (source in sources) for (rate in intArrayOf(48000, 44100)) {
            val min = AudioRecord.getMinBufferSize(rate, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT)
            if (min <= 0) continue
            var candidate: AudioRecord? = null
            try {
                candidate = AudioRecord(source, rate, AudioFormat.CHANNEL_IN_MONO,
                    AudioFormat.ENCODING_PCM_16BIT, maxOf(min, Goertzel.N * 2))
                if (candidate.state == AudioRecord.STATE_INITIALIZED) {
                    candidate.startRecording()
                    if (candidate.recordingState == AudioRecord.RECORDSTATE_RECORDING) return candidate
                }
            } catch (e: SecurityException) { candidate?.release(); throw e
            } catch (_: IllegalArgumentException) { } catch (_: IllegalStateException) { }
            try { candidate?.stop() } catch (_: IllegalStateException) { }
            candidate?.release()
        }
        error("No supported 44.1/48 kHz microphone source")
    }

    private fun acquire(audio: AudioRecord) {
        try {
            Process.setThreadPriority(Process.THREAD_PRIORITY_AUDIO)
            val rate = audio.sampleRate
            val bank = Goertzel(rate)
            val calibration = Calibration()
            val block = ShortArray(Goertzel.N)
            val hop = ShortArray(rate / 60)
            val stamp = AudioTimestamp()
            var framesRead = 0L
            var filled = 0
            var lastReport = 0L
            var calibrated = false
            var timestampFailureSince = 0L
            while (!closed.get()) {
                var read = 0
                while (read < hop.size && !closed.get()) {
                    val n = audio.read(hop, read, hop.size - read, AudioRecord.READ_BLOCKING)
                    check(n > 0) { "Microphone read failed: $n" }
                    read += n
                }
                if (closed.get()) break
                framesRead += hop.size
                val now = SystemClock.elapsedRealtimeNanos()
                // Native frame position identifies buffered/late audio, unlike read completion time.
                val timestamped = audio.getTimestamp(stamp, AudioTimestamp.TIMEBASE_BOOTTIME) == AudioRecord.SUCCESS
                val end = if (timestamped) stamp.nanoTime + (framesRead - stamp.framePosition) * 1_000_000_000L / rate else 0L
                if (!timestamped || end <= 0 || now - end !in -5_000_000L..100_000_000L) {
                    estimate = null
                    filled = 0
                    if (calibration.collecting) calibration.cancel()
                    if (timestampFailureSince == 0L) timestampFailureSince = now
                    check(now - timestampFailureSince < 2_000_000_000L) {
                        "Hardware audio timestamps unavailable or capture overrun; restart or use another device"
                    }
                    if (now - lastReport > 1_000_000_000L) {
                        report("Waiting for fresh hardware audio timestamps; no telemetry sent")
                        lastReport = now
                    }
                    continue
                }
                timestampFailureSince = 0L
                System.arraycopy(block, hop.size, block, 0, block.size - hop.size)
                System.arraycopy(hop, 0, block, block.size - hop.size, hop.size)
                filled = minOf(block.size, filled + hop.size)
                if (filled < block.size) continue
                if (calibrate.getAndSet(false)) { calibration.begin(); estimate = null; report("Calibrating: hold still with beacon on") }
                val delta = bank.shift(block)
                if (delta == null) {
                    estimate = null
                    if (calibration.collecting) {
                        calibration.cancel()
                        report("Calibration cancelled: weak or clipped signal; retry with beacon on")
                    } else if (now - lastReport > 1_000_000_000L) {
                        report("No reliable 20 kHz signal; check beacon, orientation and microphone")
                        lastReport = now
                    }
                    continue
                }
                val wasCollecting = calibration.collecting
                var calibrationFailed = false
                val corrected = try { calibration.correct(delta) } catch (e: IllegalStateException) {
                    calibrationFailed = true
                    report(e.message ?: "Calibration failed"); lastReport = now; null
                }
                if (wasCollecting && !calibration.collecting && !calibrationFailed) {
                    calibrated = true
                    report("Calibration ended; baseline %.2f Hz".format(calibration.baselineHz))
                    lastReport = now
                }
                val a = accel
                val g = gyro
                // Native timestamps can lead elapsedRealtime by a few ms; never date a packet in the future.
                val time = minOf(end, now)
                estimate = if (corrected != null && a.fresh(now) && g.fresh(now))
                    Estimate(time, (343.0 * corrected / (Goertzel.CARRIER + calibration.baselineHz)).toFloat(), a, g) else null
                if (now - lastReport > 500_000_000L && !calibration.collecting) {
                    report("${if (calibrated) "Calibrated" else "Uncalibrated"}: Δf %+.2f Hz; v %+.3f m/s\nAccel [%.2f, %.2f, %.2f] m/s²\nGyro [%.2f, %.2f, %.2f] rad/s%s".format(
                        corrected ?: 0.0, estimate?.velocity ?: 0f, a.x, a.y, a.z, g.x, g.y, g.z,
                        if (estimate == null) "\nTelemetry suppressed: stale IMU or calibration" else ""))
                    lastReport = now
                }
            }
        } catch (e: Exception) { if (!closed.get()) fail(e) }
    }

    private fun fail(error: Exception) { report("Stopped: ${error.message}"); close() }
    override fun onSensorChanged(event: SensorEvent) {
        if (event.values.size < 3) return
        val sample = Imu(event.timestamp, event.values[0], event.values[1], event.values[2])
        when (event.sensor.type) { Sensor.TYPE_ACCELEROMETER -> accel = sample; Sensor.TYPE_GYROSCOPE -> gyro = sample }
    }
    override fun onAccuracyChanged(sensor: Sensor?, accuracy: Int) = Unit

    /** Serialization prevents recorder initialization racing with resource release. */
    override fun close() {
        if (!closed.compareAndSet(false, true)) return
        estimate = null
        control.execute {
            try {
                sender.shutdownNow()
                try { channel?.close() } catch (_: java.io.IOException) { }
                sensors.unregisterListener(this)
                sensorThread.quitSafely()
                try { recorder?.stop() } catch (_: IllegalStateException) { }
                // stop() unblocks READ_BLOCKING. Never release a recorder while read() uses it.
                worker?.join()
                recorder?.release()
                recorder = null
                sender.awaitTermination(2, TimeUnit.SECONDS)
                if (sensorThread.isAlive) sensorThread.join()
            } finally { control.shutdown(); onClosed() }
        }
    }
}
