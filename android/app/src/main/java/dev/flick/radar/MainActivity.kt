package dev.flick.radar

import android.Manifest
import android.app.Activity
import android.content.*
import android.content.pm.PackageManager
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.os.Bundle
import android.provider.OpenableColumns
import android.text.InputType
import android.view.*
import android.widget.*
import java.io.ByteArrayOutputStream
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicReference

/** Dark native console; content is queued explicitly before any gesture-driven transfer. */
class MainActivity : Activity() {
    private var pipeline: RadarPipeline? = null
    private var foreground = false
    private var permissionPending = false
    private var stopping = false
    private val queued = AtomicReference<OutgoingAsset?>(null)
    private val storage = Executors.newSingleThreadExecutor()
    private lateinit var status: TextView
    private lateinit var badge: TextView
    private lateinit var payload: TextView
    private lateinit var address: EditText
    private lateinit var code: EditText
    private lateinit var start: Button
    private lateinit var stop: Button
    private lateinit var calibrate: Button
    private lateinit var radar: RadarView
    private val ink = Color.rgb(230, 248, 251)
    private val muted = Color.rgb(124, 155, 173)
    private val cyan = Color.rgb(0, 240, 255)
    private fun dp(value: Int) = (value * resources.displayMetrics.density).toInt()
    private fun surface(fill: Int, stroke: Int = Color.rgb(30, 50, 65)): GradientDrawable =
        GradientDrawable().apply { setColor(fill); cornerRadius = dp(20).toFloat(); setStroke(dp(1), stroke) }
    private fun text(resource: Int, size: Float, color: Int = ink): TextView = TextView(this).apply {
        setText(resource); textSize = size; setTextColor(color); setPadding(0, dp(5), 0, dp(5))
    }
    private fun button(resource: Int, primary: Boolean = false): Button = Button(this).apply {
        setText(resource); isAllCaps = false; textSize = 14f; minHeight = dp(50)
        setTextColor(if (primary) Color.rgb(5, 22, 30) else ink)
        background = surface(if (primary) cyan else Color.rgb(20, 35, 48), if (primary) cyan else Color.rgb(40, 67, 81))
    }
    private fun section(): LinearLayout = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL; setPadding(dp(19), dp(14), dp(19), dp(17))
        background = surface(Color.rgb(14, 25, 38))
        layoutParams = LinearLayout.LayoutParams(-1, -2).apply { topMargin = dp(14) }
    }
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (android.os.Build.VERSION.SDK_INT >= 30) window.setDecorFitsSystemWindows(false)
        val layout = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL; setPadding(dp(24), dp(20), dp(24), dp(32))
        }
        layout.addView(text(R.string.brand, 11f, cyan).apply { letterSpacing = .3f; typeface = Typeface.MONOSPACE })
        layout.addView(text(R.string.hero, 34f).apply { typeface = Typeface.create("sans-serif-light", Typeface.NORMAL) })
        layout.addView(text(R.string.hero_subtitle, 13f, muted))
        radar = RadarView(this)
        layout.addView(radar, LinearLayout.LayoutParams(-1, dp(190)))
        badge = text(R.string.idle, 10f, cyan).apply { letterSpacing = .2f; typeface = Typeface.MONOSPACE }
        layout.addView(badge)
        val connection = section()
        connection.addView(text(R.string.connection_section, 10f, muted).apply { letterSpacing = .18f })
        address = EditText(this).apply {
            setHint(R.string.desktop_hint); setTextColor(ink); setHintTextColor(muted)
            textSize = 17f; inputType = InputType.TYPE_CLASS_PHONE; setSingleLine()
            setText(getPreferences(MODE_PRIVATE).getString("desktop", ""))
        }
        code = EditText(this).apply {
            setHint(R.string.pairing_hint); setTextColor(ink); setHintTextColor(muted); textSize = 13f
            typeface = Typeface.MONOSPACE; setSingleLine()
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD
            importantForAutofill = View.IMPORTANT_FOR_AUTOFILL_NO
        }
        connection.addView(address); connection.addView(code)
        connection.addView(text(R.string.pairing_help, 11f, muted)); layout.addView(connection)
        val content = section()
        content.addView(text(R.string.payload_section, 10f, muted).apply { letterSpacing = .18f })
        payload = text(R.string.no_payload, 13f); content.addView(payload)
        val actions = LinearLayout(this)
        actions.addView(button(R.string.queue_clipboard).apply { setOnClickListener { queueClipboard() } }, LinearLayout.LayoutParams(0, dp(49), 1f))
        actions.addView(button(R.string.queue_file).apply { setOnClickListener {
            startActivityForResult(Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
                type = "*/*"; addCategory(Intent.CATEGORY_OPENABLE); addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            }, 2)
        } }, LinearLayout.LayoutParams(0, dp(49), 1f).apply { leftMargin = dp(8) })
        content.addView(actions)
        content.addView(button(R.string.clear_payload).apply { setOnClickListener {
            queued.set(null); payload.setText(R.string.no_payload); stopCapture()
        } }, LinearLayout.LayoutParams(-1, dp(43)).apply { topMargin = dp(8) })
        layout.addView(content)
        val controls = section()
        start = button(R.string.start_link, true).apply { setOnClickListener {
            if (!validAddress()) return@setOnClickListener
            if (checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
                permissionPending = true; requestPermissions(arrayOf(Manifest.permission.RECORD_AUDIO), 1)
            } else startCapture()
        } }
        stop = button(R.string.stop).apply { isEnabled = false; setOnClickListener { stopCapture() } }
        calibrate = button(R.string.calibrate).apply { isEnabled = false; setOnClickListener { pipeline?.calibrate() } }
        controls.addView(start); controls.addView(calibrate, LinearLayout.LayoutParams(-1, dp(49)).apply { topMargin = dp(8) })
        controls.addView(stop, LinearLayout.LayoutParams(-1, dp(49)).apply { topMargin = dp(8) })
        layout.addView(controls)
        status = text(R.string.initial_status, 12f, muted).apply { typeface = Typeface.MONOSPACE; setPadding(0, dp(22), 0, dp(16)) }
        layout.addView(status); layout.addView(text(R.string.footer, 10f, muted))
        setContentView(ScrollView(this).apply {
            setBackgroundColor(Color.rgb(7, 12, 20)); addView(layout)
            if (android.os.Build.VERSION.SDK_INT >= 30) setOnApplyWindowInsetsListener { view, insets ->
                val bars=insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout())
                view.setPadding(bars.left,bars.top,bars.right,bars.bottom)
                insets
            }
        })
    }
    private fun queueClipboard() {
        val clipboard = getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        val value = clipboard.primaryClip?.takeIf { it.itemCount > 0 }?.getItemAt(0)?.coerceToText(this)?.toString()
        if (value.isNullOrEmpty()) { status.setText(R.string.empty_clipboard); return }
        val bytes = value.toByteArray(Charsets.UTF_8)
        if (bytes.size > Handoff.MAX_TEXT || value.contains('\u0000')) { status.setText(R.string.payload_too_large); return }
        queued.set(OutgoingAsset(0, "", bytes)); payload.setText(R.string.clipboard_queued)
    }
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != 2 || resultCode != RESULT_OK) return
        val uri = data?.data ?: return
        status.setText(R.string.preparing_file)
        storage.execute {
            try {
                val original = contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
                    if (it.moveToFirst()) it.getString(0) else null
                } ?: "asset.bin"
                val name = original.map { if (it in 'a'..'z' || it in 'A'..'Z' || it in '0'..'9' || it in "._-") it else '_' }
                    .joinToString("").take(100).trimEnd('.').ifEmpty { "asset.bin" }
                val bytes = contentResolver.openInputStream(uri)?.use { stream ->
                    val output = ByteArrayOutputStream(); val chunk = ByteArray(8192)
                    while (!Thread.currentThread().isInterrupted) {
                        val n = stream.read(chunk); if (n < 0) break
                        check(output.size() + n <= Handoff.MAX_BYTES) { "Files must be at most 16 MiB" }
                        output.write(chunk, 0, n)
                    }
                    check(!Thread.currentThread().isInterrupted); output.toByteArray()
                } ?: error("Cannot open selected file")
                if (!isDestroyed) runOnUiThread { queued.set(OutgoingAsset(1, name, bytes)); payload.text = name; status.setText(R.string.file_ready) }
            } catch (e: Exception) { if (!isDestroyed) runOnUiThread { status.text = getString(R.string.file_error, e.message) } }
        }
    }
    private fun validAddress(): Boolean {
        val parts = address.text.toString().trim().split('.')
        val valid = parts.size == 4 && parts.all { it.isNotEmpty() && it.length <= 3 && it.all { c -> c in '0'..'9' } && it.toInt() in 0..255 } && parts[0].toInt() in 1..223 && parts[0].toInt() != 127
        if (!valid) status.setText(R.string.invalid_address)
        return valid
    }
    private fun startCapture() {
        if (!foreground || pipeline != null || !validAddress()) return
        val host = address.text.toString().trim()
        val token = try { if (code.text.isBlank()) null else Handoff.token(code.text.toString().trim()) }
            catch (_: IllegalArgumentException) { status.setText(R.string.invalid_pairing); return }
        if (queued.get() != null && token == null) { status.setText(R.string.invalid_pairing); return }
        getPreferences(MODE_PRIVATE).edit().putString("desktop", host).apply()
        lateinit var session: RadarPipeline
        val report: (String) -> Unit = { message -> runOnUiThread { if (pipeline === session && !stopping) status.text = message } }
        val transfer = token?.let { secret -> TransferClient(host, secret, { queued.get() }, { sent ->
            queued.compareAndSet(sent, null)
            runOnUiThread { if (queued.get() == null) payload.setText(R.string.no_payload) }
        }, report) }
        session = RadarPipeline(applicationContext, host, transfer, report, { runOnUiThread {
            if (pipeline === session) {
                pipeline = null; stopping = false; start.isEnabled = true; address.isEnabled = true; code.isEnabled = true
                stop.isEnabled = false; calibrate.isEnabled = false; badge.setText(R.string.idle); radar.linked = false
                window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
            }
        } })
        pipeline = session; stopping = false; start.isEnabled = false; address.isEnabled = false; code.isEnabled = false
        stop.isEnabled = true; calibrate.isEnabled = true; badge.setText(R.string.listening); radar.linked = true
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON); status.setText(R.string.starting); session.start()
    }
    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode != 1) return
        val wanted = permissionPending; permissionPending = false
        if (foreground && wanted && grantResults.firstOrNull() == PackageManager.PERMISSION_GRANTED) startCapture()
        else if (foreground) status.setText(R.string.permission_required)
    }
    private fun stopCapture() {
        permissionPending = false
        if (pipeline == null || stopping) return
        stopping = true; stop.isEnabled = false; calibrate.isEnabled = false; status.setText(R.string.stopping); pipeline?.close()
    }
    override fun onStart() { super.onStart(); foreground = true; if (::radar.isInitialized) radar.animate = true }
    override fun onStop() { foreground = false; if (::radar.isInitialized) radar.animate = false; stopCapture(); window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON); super.onStop() }
    override fun onDestroy() { storage.shutdownNow(); queued.set(null); super.onDestroy() }
}
