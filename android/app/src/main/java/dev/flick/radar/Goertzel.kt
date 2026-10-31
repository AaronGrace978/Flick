package dev.flick.radar

import kotlin.math.*

/** Hann-windowed Goertzel bank, spaced at half-bin intervals around 20 kHz.
 * N=4096 gives 11.72 Hz independent resolution at 48 kHz (92 ms at 44.1 kHz).
 * Overlap raises update rate, not physical frequency resolution.
 */
class Goertzel(private val rate: Int) {
    companion object { const val N = 4096; const val CARRIER = 20000.0 }
    private val spacing = rate.toDouble() / N / 2
    private val window = DoubleArray(N) { 0.5 - 0.5 * cos(2 * PI * it / (N - 1)) }
    private val coefficients = DoubleArray(41) { 2 * cos(2 * PI * (CARRIER + (it - 20) * spacing) / rate) }
    private val work = DoubleArray(N)
    private val powers = DoubleArray(41)

    /** Returns signed observed frequency shift, or null for weak/ambiguous audio.
     * Thresholds are conservative POC defaults and require hardware calibration.
     */
    fun shift(samples: ShortArray): Double? {
        var energy = 0.0
        for (i in 0 until N) {
            val x = samples[i] / 32768.0 * window[i]
            work[i] = x
            energy += x * x
        }
        if (energy < 1e-6) return null
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
        if (powers[peak] / (energy * N) < 0.08) return null
        var floor = 0.0
        var count = 0
        for (i in powers.indices) if (abs(i - peak) > 4) { floor += powers[i]; count++ }
        if (powers[peak] < 10 * floor / count) return null
        val left = ln(max(powers[peak - 1], 1e-30))
        val center = ln(max(powers[peak], 1e-30))
        val right = ln(max(powers[peak + 1], 1e-30))
        val denominator = left - 2 * center + right
        val offset = if (abs(denominator) > 1e-12) (0.5 * (left - right) / denominator).coerceIn(-0.5, 0.5) else 0.0
        return (peak - 20 + offset) * spacing
    }
}
