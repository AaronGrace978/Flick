package dev.flick.radar

import android.content.Context
import android.graphics.*
import android.view.View
import kotlin.math.*

/** Decorative sensor-console artwork. Rings do not represent a measured position. */
class RadarView(context: Context) : View(context) {
    var linked = false
    var animate = true
        set(value) { field = value; if (value) invalidate() }
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG)
    private var phase = 0f
    private val arrow = Path()
    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        val cx = width / 2f; val cy = height / 2f; val r = min(width, height) * .4f
        paint.style = Paint.Style.STROKE; paint.strokeWidth = resources.displayMetrics.density
        for (i in 1..4) { paint.color = Color.argb(30 + i * 7, 0, 240, 255); canvas.drawCircle(cx, cy, r * i / 4, paint) }
        paint.color = Color.rgb(24, 45, 59)
        canvas.drawLine(cx - r - 20, cy, cx + r + 20, cy, paint);canvas.drawLine(cx, cy - r - 14, cx, cy + r + 14, paint)
        paint.color = Color.rgb(0, 240, 255);paint.strokeWidth = resources.displayMetrics.density * 2
        canvas.drawArc(cx - r, cy - r, cx + r, cy + r, -110f + phase, 57f, false, paint)
        paint.color = Color.argb(90, 0, 240, 255);canvas.drawArc(cx - r * .75f, cy - r * .75f, cx + r * .75f, cy + r * .75f, 60f - phase, 33f, false, paint)
        paint.style = Paint.Style.FILL;paint.color = Color.rgb(8, 24, 36);canvas.drawCircle(cx, cy, r * .38f, paint)
        paint.color = if (linked) Color.rgb(131, 255, 216) else Color.rgb(0, 240, 255)
        arrow.reset()
        arrow.apply { moveTo(cx - 8, cy - 19);lineTo(cx + 12, cy - 19);lineTo(cx - 2, cy - 2);lineTo(cx + 10, cy - 2);lineTo(cx - 12, cy + 23);lineTo(cx - 4, cy + 3);lineTo(cx - 15, cy + 3);close() }
        canvas.drawPath(arrow, paint)
        if (animate) { phase = (phase + if (linked) .7f else .16f) % 360; postInvalidateDelayed(33) }
    }
}
