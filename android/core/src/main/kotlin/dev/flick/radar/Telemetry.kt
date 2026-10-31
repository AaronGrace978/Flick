package dev.flick.radar

import java.nio.ByteBuffer
import java.nio.ByteOrder

/** Immutable native-device-axis IMU sample, timestamped in elapsed-realtime ns. */
data class Imu(val time: Long = 0, val x: Float = 0f, val y: Float = 0f, val z: Float = 0f) {
    fun fresh(now: Long) = time > 0 && now - time in 0..100_000_000L &&
        x.isFinite() && y.isFinite() && z.isFinite()
}

/** v1 packet: acquisition-window end timestamp, signed velocity and raw acceleration. */
data class Estimate(val time: Long, val velocity: Float, val accel: Imu, val gyro: Imu)

object Telemetry {
    const val BYTES = 24
    const val PERIOD_NS = 1_000_000_000L / 60
    fun encode(buffer: ByteBuffer, sample: Estimate) {
        require(buffer.capacity() >= BYTES && sample.time > 0 && sample.velocity.isFinite())
        require(sample.accel.x.isFinite() && sample.accel.y.isFinite() && sample.accel.z.isFinite())
        buffer.clear().limit(BYTES)
        buffer.order(ByteOrder.BIG_ENDIAN).putLong(sample.time).putFloat(sample.velocity)
            .putFloat(sample.accel.x).putFloat(sample.accel.y).putFloat(sample.accel.z)
        buffer.flip()
    }
    /** Advance to the next future slot, skipping missed periods without catch-up bursts. */
    fun nextDeadline(previous: Long, now: Long): Long {
        var next = previous + PERIOD_NS
        if (next <= now) next += ((now - next) / PERIOD_NS + 1) * PERIOD_NS
        return next
    }
    /** Never replay missed scheduler deadlines or enqueue old measurements. */
    fun eligible(sample: Estimate?, sent: Long, now: Long): Boolean =
        sample != null && sample.time > sent && now - sample.time in 0..100_000_000L
}

/** Median stationary clock-bias calibration. Confined to the DSP thread.
 * Motion during calibration invalidates its physical meaning; range is still unknown.
 */
class Calibration {
    private val samples = DoubleArray(60)
    private var count = -1
    var baselineHz = 0.0
        private set
    val collecting get() = count >= 0
    fun begin() { count = 0 }
    fun cancel() { count = -1 }
    fun correct(shift: Double): Double? {
        if (!shift.isFinite()) return null
        if (count >= 0) {
            samples[count++] = shift
            if (count == samples.size) {
                samples.sort()
                // Reject movement/drift rather than silently calibrating a wide distribution.
                if (samples[54] - samples[5] <= 3.0) baselineHz = (samples[29] + samples[30]) / 2
                else { count = -1; throw IllegalStateException("Calibration unstable; hold the phone stationary and retry") }
                count = -1
            }
            return null
        }
        return shift - baselineHz
    }
}
