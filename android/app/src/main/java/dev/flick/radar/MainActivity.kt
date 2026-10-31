package dev.flick.radar

import android.Manifest
import android.app.Activity
import android.content.pm.PackageManager
import android.os.Bundle
import android.view.WindowManager
import android.widget.*

/** Native foreground controls. Permission callbacks never restart a hidden activity. */
class MainActivity : Activity() {
    private var pipeline: RadarPipeline? = null
    private var foreground = false
    private var permissionPending = false
    private var stopping = false
    private lateinit var status: TextView
    private lateinit var address: EditText
    private lateinit var start: Button
    private lateinit var stop: Button
    private lateinit var calibrate: Button

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val padding = (20 * resources.displayMetrics.density).toInt()
        val layout = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; setPadding(padding, padding, padding, padding) }
        layout.addView(TextView(this).apply { text = getString(R.string.intro) })
        address = EditText(this).apply {
            hint = getString(R.string.desktop_hint)
            inputType = android.text.InputType.TYPE_CLASS_PHONE
            setSingleLine()
            setText(getPreferences(MODE_PRIVATE).getString("desktop", ""))
        }
        status = TextView(this).apply { text = getString(R.string.initial_status) }
        start = Button(this).apply { text = getString(R.string.start); setOnClickListener {
            if (!validAddress()) return@setOnClickListener
            if (checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
                permissionPending = true
                requestPermissions(arrayOf(Manifest.permission.RECORD_AUDIO), 1)
            } else startCapture()
        } }
        stop = Button(this).apply { text = getString(R.string.stop); isEnabled = false; setOnClickListener { stopCapture() } }
        calibrate = Button(this).apply { text = getString(R.string.calibrate); isEnabled = false; setOnClickListener { pipeline?.calibrate() } }
        layout.addView(address); layout.addView(start); layout.addView(calibrate); layout.addView(stop); layout.addView(status)
        setContentView(ScrollView(this).apply { addView(layout) })
    }

    private fun validAddress(): Boolean {
        val host = address.text.toString().trim()
        val parts = host.split('.')
        val valid = parts.size == 4 && parts.all { part ->
            part.isNotEmpty() && part.length <= 3 && part.all { it in '0'..'9' } && part.toInt() in 0..255
        } && parts[0].toInt() in 1..223 && parts[0].toInt() != 127
        if (!valid) status.text = getString(R.string.invalid_address)
        return valid
    }

    private fun startCapture() {
        if (!foreground || pipeline != null || !validAddress()) return
        val host = address.text.toString().trim()
        getPreferences(MODE_PRIVATE).edit().putString("desktop", host).apply()
        lateinit var session: RadarPipeline
        session = RadarPipeline(applicationContext, host, { message -> runOnUiThread {
            if (pipeline === session && !stopping) status.text = message
        } }, { runOnUiThread {
            if (pipeline === session) {
                pipeline = null
                stopping = false
                start.isEnabled = true; address.isEnabled = true
                stop.isEnabled = false; calibrate.isEnabled = false
                window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
            }
        } })
        pipeline = session
        stopping = false
        start.isEnabled = false; address.isEnabled = false
        stop.isEnabled = true; calibrate.isEnabled = true
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        status.text = getString(R.string.starting)
        session.start()
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode != 1) return
        val wanted = permissionPending
        permissionPending = false
        if (foreground && wanted && grantResults.firstOrNull() == PackageManager.PERMISSION_GRANTED) startCapture()
        else if (foreground) status.text = getString(R.string.permission_required)
    }
    private fun stopCapture() {
        permissionPending = false
        if (pipeline == null || stopping) return
        stopping = true
        stop.isEnabled = false; calibrate.isEnabled = false
        status.text = getString(R.string.stopping)
        pipeline?.close()
    }
    override fun onStart() { super.onStart(); foreground = true }
    override fun onStop() {
        foreground = false
        stopCapture()
        window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        super.onStop()
    }
}
