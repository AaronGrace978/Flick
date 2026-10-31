package dev.flick.radar

import android.Manifest
import android.app.Activity
import android.content.pm.PackageManager
import android.os.Bundle
import android.widget.*
import java.net.Inet4Address
import java.net.InetAddress

/** Minimal native UI; capture stops when the activity leaves the foreground. */
class MainActivity : Activity() {
    private var pipeline: RadarPipeline? = null
    private lateinit var status: TextView
    private lateinit var address: EditText
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val layout = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; setPadding(32, 48, 32, 32) }
        address = EditText(this).apply { hint = "Desktop LAN IPv4 address"; inputType = android.text.InputType.TYPE_CLASS_PHONE }
        status = TextView(this).apply { text = "Use low speaker volume. 20 kHz may be audible. No absolute distance measurement." }
        layout.addView(address)
        layout.addView(Button(this).apply { text = "Start"; setOnClickListener {
            if (checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED)
                requestPermissions(arrayOf(Manifest.permission.RECORD_AUDIO), 1)
            else startCapture()
        } })
        layout.addView(Button(this).apply { text = "Stop"; setOnClickListener { stopCapture() } })
        layout.addView(status)
        setContentView(layout)
    }
    private fun startCapture() {
        if (pipeline != null) return
        val host = address.text.toString().trim()
        if (!host.matches(Regex("[0-9]{1,3}(\\.[0-9]{1,3}){3}")) || host.split('.').any { it.toInt() > 255 }) {
            status.text = "Enter a valid desktop IPv4 address"; return
        }
        try {
            pipeline = RadarPipeline(this, host) { message -> runOnUiThread { status.text = message } }.also { it.start() }
        } catch (e: Exception) { pipeline = null; status.text = "Cannot start: ${e.message}" }
    }
    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode == 1 && grantResults.firstOrNull() == PackageManager.PERMISSION_GRANTED) startCapture()
    }
    private fun stopCapture() { pipeline?.close(); pipeline = null; status.text = "Stopped" }
    override fun onStop() { stopCapture(); super.onStop() }
}
