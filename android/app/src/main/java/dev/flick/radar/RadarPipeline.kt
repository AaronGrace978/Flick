package dev.flick.radar

import android.content.Context
import android.hardware.*
import android.media.*
import android.os.*
import java.net.InetAddress
import java.net.InetSocketAddress
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.nio.channels.DatagramChannel
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

/** Foreground-only acquisition. All native resources are released by close().
 * Raw acceleration includes gravity and uses Android device axes, not world axes.
 * IMU readings are bundled with acoustic velocity; without range/attitude calibration
 * they cannot provide a defensible absolute position or inertial drift correction.
 */
class RadarPipeline(context: Context, private val host: String, private val report: (String) -> Unit) : AutoCloseable, SensorEventListener {
    data class Imu(val time: Long = 0, val x: Float = 0f, val y: Float = 0f, val z: Float = 0f)
    data class Estimate(val time: Long, val velocity: Float, val accel: Imu, val gyro: Imu)
    private val sensors = context.getSystemService(Context.SENSOR_SERVICE) as SensorManager
    private val sensorThread = HandlerThread("flick-imu")
    private val running = AtomicBoolean(false)
    private val scheduler = Executors.newSingleThreadScheduledExecutor()
    @Volatile private var accel = Imu()
    @Volatile private var gyro = Imu()
    @Volatile private var estimate: Estimate? = null
    private var recorder: AudioRecord? = null
    private var worker: Thread? = null
    private var channel: DatagramChannel? = null

    /** Requires RECORD_AUDIO permission. Audio is unprocessed when supported. */
    fun start() {
        check(running.compareAndSet(false, true))
        try {
            sensorThread.start()
            for (type in intArrayOf(Sensor.TYPE_ACCELEROMETER, Sensor.TYPE_GYROSCOPE)) {
                val sensor = sensors.getDefaultSensor(type) ?: error("Required IMU sensor unavailable: $type")
                check(sensors.registerListener(this, sensor, 5000, Handler(sensorThread.looper)))
            }
            val source = if (contextAudioUnprocessed()) MediaRecorder.AudioSource.UNPROCESSED else MediaRecorder.AudioSource.VOICE_RECOGNITION
            var rate = 0
            for (candidate in intArrayOf(48000, 44100)) {
                val min = AudioRecord.getMinBufferSize(candidate, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT)
                if (min <= 0) continue
                val r = AudioRecord(source, candidate, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT, maxOf(min, Goertzel.N * 4))
                if (r.state == AudioRecord.STATE_INITIALIZED) { recorder = r; rate = candidate; break }
                r.release()
            }
            val audio = recorder ?: error("No 44.1/48 kHz microphone configuration")
            audio.startRecording()
            check(audio.recordingState == AudioRecord.RECORDSTATE_RECORDING)
            worker = Thread({ acquire(audio, rate) }, "flick-audio").also { it.start() }
            // DNS and channel setup happen off the UI thread.
            scheduler.execute {
                try {
                    val destination = InetSocketAddress(InetAddress.getByName(host), 8089)
                    val udp = DatagramChannel.open()
                    channel = udp
                    udp.configureBlocking(false)
                    val buffer = ByteBuffer.allocate(24).order(ByteOrder.BIG_ENDIAN)
                    var sent = 0L
                    scheduler.scheduleAtFixedRate({
                        try {
                            val sample = estimate
                            val now = SystemClock.elapsedRealtimeNanos()
                            if (sample != null && sample.time > sent && now - sample.time < 100_000_000L) {
                                buffer.clear()
                                buffer.putLong(sample.time).putFloat(sample.velocity)
                                    .putFloat(sample.accel.x).putFloat(sample.accel.y).putFloat(sample.accel.z).flip()
                                // Zero means would-block: drop this datagram rather than queueing it.
                                udp.send(buffer, destination)
                                sent = sample.time
                            }
                        } catch (e: Exception) { fail(e) }
                    }, 0, 16_666_667, TimeUnit.NANOSECONDS)
                    report("Recording at $rate Hz; UDP to $host:8089")
                } catch (e: Exception) { fail(e) }
            }
        } catch (e: Exception) { close(); throw e }
    }
    private val audioManager = context.getSystemService(Context.AUDIO_SERVICE) as AudioManager
    private fun contextAudioUnprocessed() = audioManager.getProperty(AudioManager.PROPERTY_SUPPORT_AUDIO_SOURCE_UNPROCESSED) == "true"

    private fun acquire(audio: AudioRecord, rate: Int) {
        Process.setThreadPriority(Process.THREAD_PRIORITY_AUDIO)
        val bank = Goertzel(rate)
        val block = ShortArray(Goertzel.N)
        val hop = ShortArray(rate / 60)
        var filled = 0
        try {
            while (running.get()) {
                var read = 0
                while (read < hop.size && running.get()) {
                    val n = audio.read(hop, read, hop.size - read, AudioRecord.READ_BLOCKING)
                    check(n > 0) { "AudioRecord read failed: $n" }
                    read += n
                }
                if (!running.get()) break
                System.arraycopy(block, hop.size, block, 0, block.size - hop.size)
                System.arraycopy(hop, 0, block, block.size - hop.size, hop.size)
                filled += hop.size
                if (filled < block.size) continue
                filled = block.size
                val delta = bank.shift(block)
                val now = SystemClock.elapsedRealtimeNanos()
                val a = accel
                val g = gyro
                estimate = if (delta != null && now - a.time in 0..100_000_000L && now - g.time in 0..100_000_000L)
                    Estimate(now, (343.0 * delta / Goertzel.CARRIER).toFloat(), a, g) else null
            }
        } catch (e: Exception) { if (running.get()) fail(e) }
    }
    private fun fail(e: Exception) { if (running.getAndSet(false)) report("Pipeline stopped: ${e.message}. Tap Stop to release resources.") }
    override fun onSensorChanged(event: SensorEvent) {
        val sample = Imu(event.timestamp, event.values[0], event.values[1], event.values[2])
        when (event.sensor.type) { Sensor.TYPE_ACCELEROMETER -> accel = sample; Sensor.TYPE_GYROSCOPE -> gyro = sample }
    }
    override fun onAccuracyChanged(sensor: Sensor?, accuracy: Int) = Unit
    /** Stop capture before joining, to unblock a pending native read. */
    override fun close() {
        running.set(false)
        scheduler.shutdownNow()
        sensors.unregisterListener(this)
        sensorThread.quitSafely()
        try { recorder?.stop() } catch (_: IllegalStateException) { }
        worker?.join(2000)
        recorder?.release()
        recorder = null
        scheduler.awaitTermination(2, TimeUnit.SECONDS)
        channel?.close()
    }
}
