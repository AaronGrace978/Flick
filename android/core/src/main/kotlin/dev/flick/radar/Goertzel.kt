package dev.flick.radar

import kotlin.math.*

/** Hann-windowed Goertzel bank, spaced at half-bin intervals around 20 kHz.
 * N=4096 gives 11.72 Hz independent resolution at 48 kHz (92 ms at 44.1 kHz).
 * Overlap raises update rate, not physical frequency resolution.
 */
class Goertzel(private val rate: Int) {
    companion object {
        const val N = 4096
        const val CARRIER = 20000.0
        /** ±50 half-bins: about ±293 Hz (±5 m/s) at 48 kHz, ±269 Hz at 44.1 kHz. */
        const val BINS = 101
        private const val CENTER = BINS / 2
    }
    init { require(rate == 44100 || rate == 48000) }
    private val spacing = rate.toDouble() / N / 2
    private val window = DoubleArray(N) { 0.5 - 0.5 * cos(2 * PI * it / (N - 1)) }
    private val coefficients = DoubleArray(BINS) { 2 * cos(2 * PI * (CARRIER + (it - CENTER) * spacing) / rate) }
    private val work = DoubleArray(N)
    private val powers = DoubleArray(BINS)

    /** Returns signed observed frequency shift, or null for weak/ambiguous audio.
     * Confidence is judged against the in-band floor only. Broadband room noise
     * (speech, fans, mains hum) is far louder than a distant beacon but does not
     * overlap the ultrasonic band, so it must not veto an otherwise clean tone.
     */
    fun shift(samples: ShortArray): Double? {
        require(samples.size == N)
        var clipped = 0
        var energy = 0.0
        for (i in 0 until N) {
            if (abs(samples[i].toInt()) >= 32760) clipped++
            val x = samples[i] / 32768.0 * window[i]
            work[i] = x
            energy += x * x
        }
        if (clipped > N / 100 || energy < 1e-9) return null
        for (j in coefficients.indices) {
            val c = coefficients[j]
            var q1 = 0.0
            var q2 = 0.0
            for (x in work) { val q = x + c * q1 - q2; q2 = q1; q1 = q }
            powers[j] = max(0.0, q1 * q1 + q2 * q2 - c * q1 * q2)
        }
        var peak = 0
        for (i in powers.indices) if (powers[i] > powers[peak]) peak = i
        if (peak == 0 || peak == powers.lastIndex) return null
        // Distinct strong secondary peaks indicate competing tones/multipath ambiguity.
        for (i in 1 until powers.lastIndex) {
            if (abs(i - peak) > 4 && powers[i] >= powers[i - 1] && powers[i] >= powers[i + 1]
                && powers[i] > powers[peak] * 0.4) return null
        }
        var floor = 0.0
        var count = 0
        for (i in powers.indices) if (abs(i - peak) > 4) { floor += powers[i]; count++ }
        if (powers[peak] < 12 * floor / count) return null
        val left = ln(max(powers[peak - 1], 1e-30))
        val center = ln(max(powers[peak], 1e-30))
        val right = ln(max(powers[peak + 1], 1e-30))
        val denominator = left - 2 * center + right
        val offset = if (abs(denominator) > 1e-12) (0.5 * (left - right) / denominator).coerceIn(-0.5, 0.5) else 0.0
        return (peak - CENTER + offset) * spacing
    }
}
