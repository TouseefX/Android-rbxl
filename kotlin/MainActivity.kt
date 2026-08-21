package com.yourname.rbxleditor

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.media.AudioAttributes
import android.media.MediaPlayer
import android.net.Uri
import android.os.Bundle
import android.os.Environment
import android.os.Looper
import android.os.StrictMode
import android.util.Log
import android.view.View
import android.view.WindowManager
import android.widget.Toast
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import com.google.androidgamesdk.GameActivity
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream
import java.nio.charset.StandardCharsets
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/**
 * Host Activity for the Rust/Bevy editor.
 *
 * It extends [GameActivity] (androidx.games:games-activity) rather than
 * `NativeActivity`, which is what gives us GameTextInput: a real
 * `InputConnection`, so Gboard / Samsung keyboard editing *and paste* work in
 * the egui text fields. The Rust side must be built with the
 * `android-game-activity` backend (see `Cargo.toml`) so that
 * `android-activity`'s GameActivity glue is linked in.
 *
 * Everything below the lifecycle methods is the JNI surface used by
 * `src/jni_bridge.rs`:
 *  - `*Static` helpers are called *from* Rust (`call_static_method`), so their
 *    names and JVM signatures must stay in sync with `jni_bridge.rs`.
 *  - `native*` methods are *implemented in* Rust
 *    (`Java_com_yourname_rbxleditor_MainActivity_*`).
 */
class MainActivity : GameActivity() {

    private var currentDocUri: Uri? = null

    // External edit state
    private var activeExternalScriptId: Long = -1
    private var activeExternalFilePath: String? = null
    private var lastExternalModifiedTime: Long = 0

    override fun onCreate(savedInstanceState: Bundle?) {
        // Render behind the system bars; GameActivity reports the insets to the
        // native side so Bevy/egui can lay out around the cutout.
        WindowCompat.setDecorFitsSystemWindows(window, false)
        hideSystemUi()
        super.onCreate(savedInstanceState)
        sInstance = this

        // Allow handing a plain file:// Uri to external editor apps without
        // tripping FileUriExposedException.
        try {
            StrictMode.setVmPolicy(StrictMode.VmPolicy.Builder().build())
        } catch (e: Exception) {
            Log.w(TAG, "StrictMode config exception", e)
        }

        Log.i(TAG, "MainActivity onCreate: instance registered")
    }

    override fun onResume() {
        super.onResume()
        hideSystemUi()
        // Whenever the user switches back to the app, auto-sync any script that
        // was modified in an external editor.
        checkExternalFileUpdate(false)
    }

    private fun hideSystemUi() {
        try {
            window.attributes.layoutInDisplayCutoutMode =
                WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_ALWAYS
            val decorView: View = window.decorView
            val controller = WindowInsetsControllerCompat(window, decorView)
            controller.hide(WindowInsetsCompat.Type.systemBars())
            controller.hide(WindowInsetsCompat.Type.displayCutout())
            controller.systemBarsBehavior =
                WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        } catch (e: Exception) {
            Log.w(TAG, "hideSystemUi failed", e)
        }
    }

    // ---- Instance methods, all running on the Android UI thread -------------

    fun copyToClipboard(text: String) {
        // ClipboardManager must be touched on the UI thread. GameActivity input
        // callbacks already run there, so posting + waiting would deadlock; run
        // inline when we're already on it.
        if (Looper.myLooper() == Looper.getMainLooper()) {
            doCopyToClipboard(text)
        } else {
            runOnUiThread { doCopyToClipboard(text) }
        }
    }

    private fun doCopyToClipboard(text: String) {
        try {
            val clipboard = getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
            clipboard?.setPrimaryClip(ClipData.newPlainText("RobloxSnippet", text))
        } catch (e: Exception) {
            Log.e(TAG, "copyToClipboard failed", e)
        }
    }

    fun getClipboardText(): String {
        if (Looper.myLooper() == Looper.getMainLooper()) {
            return doGetClipboardText()
        }
        val result = arrayOf("")
        val latch = CountDownLatch(1)
        runOnUiThread {
            result[0] = doGetClipboardText()
            latch.countDown()
        }
        try {
            latch.await(1, TimeUnit.SECONDS)
        } catch (e: InterruptedException) {
            Log.w(TAG, "getClipboardText interrupted")
        }
        return result[0]
    }

    private fun doGetClipboardText(): String {
        try {
            val clipboard = getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
            if (clipboard != null && clipboard.hasPrimaryClip()) {
                val clip = clipboard.primaryClip
                if (clip != null && clip.itemCount > 0) {
                    val text = clip.getItemAt(0).coerceToText(this)
                    if (text != null) return text.toString()
                }
            }
        } catch (e: Exception) {
            Log.e(TAG, "getClipboardText failed", e)
        }
        return ""
    }

    fun openDocument() {
        runOnUiThread {
            try {
                val intent = Intent(Intent.ACTION_OPEN_DOCUMENT)
                intent.addCategory(Intent.CATEGORY_OPENABLE)
                intent.type = "*/*"
                intent.addFlags(
                    Intent.FLAG_GRANT_READ_URI_PERMISSION or
                        Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION
                )
                startActivityForResult(intent, REQ_OPEN)
            } catch (e: Exception) {
                Log.e(TAG, "openDocument startActivityForResult failed", e)
                nativeOnDocumentOpened(null, null)
            }
        }
    }

    /**
     * Launch the Storage Access Framework picker for a local Roblox MODEL file
     * (.rbxm binary or .rbxmx XML). Unlike [openDocument] this does NOT replace
     * the currently-open place — the bytes come back on the REQ_OPEN_MODEL
     * channel and are inserted as a subtree into the active place.
     *
     * We don't rely on a single MIME type because file managers report wildly
     * different types for .rbxm/.rbxmx, so we accept all openable documents and
     * let the Rust decoder validate the payload by magic bytes.
     */
    fun openModelDocument() {
        runOnUiThread {
            try {
                val intent = Intent(Intent.ACTION_OPEN_DOCUMENT)
                intent.addCategory(Intent.CATEGORY_OPENABLE)
                intent.type = "*/*"
                intent.putExtra(
                    Intent.EXTRA_MIME_TYPES,
                    arrayOf(
                        "application/octet-stream",
                        "application/x-rbxm",
                        "application/x-roblox",
                        "model/rbxm",
                        "text/xml",
                        "application/xml",
                    )
                )
                intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                startActivityForResult(intent, REQ_OPEN_MODEL)
            } catch (e: Exception) {
                Log.e(TAG, "openModelDocument startActivityForResult failed", e)
                nativeOnModelOpened(null, null)
            }
        }
    }

    fun createDocument(suggestedName: String) {
        runOnUiThread {
            try {
                val intent = Intent(Intent.ACTION_CREATE_DOCUMENT)
                intent.addCategory(Intent.CATEGORY_OPENABLE)
                intent.type = "application/octet-stream"
                intent.putExtra(Intent.EXTRA_TITLE, suggestedName)
                startActivityForResult(intent, REQ_CREATE)
            } catch (e: Exception) {
                Log.e(TAG, "createDocument failed", e)
            }
        }
    }

    fun saveToCurrentDocument(data: ByteArray) {
        runOnUiThread {
            val uri = currentDocUri
            if (uri == null) {
                nativeOnSaveComplete(false)
            } else {
                nativeOnSaveComplete(writeBytes(uri, data))
            }
        }
    }

    /**
     * Writes a real .lua file into Android/media/ (which stays reachable by
     * third-party editors on Android 13+, unlike Android/data) and hands it to
     * whatever editor app the user picks.
     */
    fun editExternally(scriptId: Long, fileName: String, source: String) {
        runOnUiThread {
            try {
                var scriptsDir: File? = null
                val primaryMediaDir = externalMediaDirs.firstOrNull()
                if (primaryMediaDir != null) {
                    scriptsDir = File(primaryMediaDir, "scripts")
                }
                if (scriptsDir == null || !scriptsDir.exists()) {
                    val mediaRoot = File(
                        Environment.getExternalStorageDirectory(),
                        "Android/media/com.yourname.rbxleditor/scripts"
                    )
                    if (mediaRoot.exists() || mediaRoot.mkdirs()) {
                        scriptsDir = mediaRoot
                    }
                }
                val targetDir = scriptsDir ?: File(getExternalFilesDir(null) ?: filesDir, "scripts")
                if (!targetDir.exists()) {
                    targetDir.mkdirs()
                }

                var sanitized = fileName.replace(Regex("[^a-zA-Z0-9_.-]"), "_")
                if (!sanitized.endsWith(".lua")) {
                    sanitized += ".lua"
                }

                val scriptFile = File(targetDir, sanitized)
                FileOutputStream(scriptFile).use { fos ->
                    fos.write(source.toByteArray(StandardCharsets.UTF_8))
                    fos.flush()
                }

                activeExternalScriptId = scriptId
                activeExternalFilePath = scriptFile.absolutePath
                lastExternalModifiedTime = scriptFile.lastModified()

                val fileUri = Uri.fromFile(scriptFile)
                val editIntent = Intent(Intent.ACTION_VIEW)
                editIntent.setDataAndType(fileUri, "text/plain")
                editIntent.addFlags(
                    Intent.FLAG_GRANT_READ_URI_PERMISSION or
                        Intent.FLAG_GRANT_WRITE_URI_PERMISSION or
                        Intent.FLAG_ACTIVITY_NEW_TASK
                )

                val chooser = Intent.createChooser(editIntent, "Edit Lua script with...")
                chooser.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                startActivity(chooser)

                Log.i(TAG, "editExternally: launched chooser for ${scriptFile.absolutePath}")
            } catch (e: Exception) {
                Log.e(TAG, "editExternally failed", e)
            }
        }
    }

    fun checkExternalFileUpdate(force: Boolean) {
        try {
            val path = activeExternalFilePath ?: return
            if (activeExternalScriptId < 0) return
            val file = File(path)
            if (!file.exists()) return
            val currentMod = file.lastModified()
            if (force || currentMod > lastExternalModifiedTime) {
                lastExternalModifiedTime = currentMod
                val bytes = readFile(file) ?: return
                val text = String(bytes, StandardCharsets.UTF_8)
                Log.i(TAG, "checkExternalFileUpdate: syncing ${text.length} chars from ${file.name}")
                nativeOnExternalEditReturned(activeExternalScriptId, text)
            }
        } catch (e: Exception) {
            Log.e(TAG, "checkExternalFileUpdate exception", e)
        }
    }

    fun finishExternalEdit() {
        activeExternalScriptId = -1
        activeExternalFilePath = null
        lastExternalModifiedTime = 0
    }

    private fun launchViewer(packageName: String) {
        try {
            val i = packageManager.getLaunchIntentForPackage(packageName)
            if (i == null) {
                Log.e(TAG, "launchViewer: no launch intent for $packageName")
                return
            }
            i.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            startActivity(i)
        } catch (e: Exception) {
            Log.e(TAG, "launchViewer failed", e)
        }
    }

    @Suppress("DEPRECATION")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)

        try {
            when (requestCode) {
                REQ_OPEN -> {
                    val uri = if (resultCode == RESULT_OK) data?.data else null
                    if (uri == null) {
                        nativeOnDocumentOpened(null, null)
                        return
                    }
                    persist(uri)
                    currentDocUri = uri
                    nativeOnDocumentOpened(uri.toString(), readBytes(uri))
                }

                REQ_OPEN_MODEL -> {
                    // A local .rbxm/.rbxmx: do NOT touch currentDocUri (that is
                    // the open place's save target); just hand the bytes to Rust
                    // to decode and insert as a subtree.
                    val uri = if (resultCode == RESULT_OK) data?.data else null
                    if (uri == null) {
                        nativeOnModelOpened(null, null)
                        return
                    }
                    try {
                        contentResolver.takePersistableUriPermission(
                            uri,
                            Intent.FLAG_GRANT_READ_URI_PERMISSION
                        )
                    } catch (e: Exception) {
                        Log.w(TAG, "takePersistableUriPermission (read) failed (non-fatal)", e)
                    }
                    nativeOnModelOpened(uri.toString(), readBytes(uri))
                }

                REQ_CREATE -> {
                    val uri = if (resultCode == RESULT_OK) data?.data else null
                    if (uri == null) return
                    persist(uri)
                    currentDocUri = uri
                    nativeOnDocumentCreated(uri.toString())
                }
            }
        } catch (e: Exception) {
            Log.e(TAG, "onActivityResult exception", e)
        }
    }

    private fun persist(uri: Uri) {
        try {
            contentResolver.takePersistableUriPermission(
                uri,
                Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
            )
        } catch (e: Exception) {
            Log.w(TAG, "takePersistableUriPermission failed (non-fatal)", e)
        }
    }

    private fun readBytes(uri: Uri): ByteArray? {
        return try {
            contentResolver.openInputStream(uri)?.use { input ->
                val buf = ByteArrayOutputStream()
                val chunk = ByteArray(8192)
                while (true) {
                    val n = input.read(chunk)
                    if (n == -1) break
                    buf.write(chunk, 0, n)
                }
                buf.toByteArray()
            }
        } catch (e: Exception) {
            Log.e(TAG, "readBytes failed", e)
            null
        }
    }

    private fun readFile(file: File): ByteArray? {
        return try {
            FileInputStream(file).use { input ->
                val buf = ByteArrayOutputStream()
                val chunk = ByteArray(8192)
                while (true) {
                    val n = input.read(chunk)
                    if (n == -1) break
                    buf.write(chunk, 0, n)
                }
                buf.toByteArray()
            }
        } catch (e: Exception) {
            Log.e(TAG, "readFile failed: $file", e)
            null
        }
    }

    private fun writeBytes(uri: Uri, data: ByteArray): Boolean {
        return try {
            contentResolver.openOutputStream(uri, "wt")?.use { out ->
                out.write(data)
                out.flush()
                true
            } ?: false
        } catch (e: Exception) {
            Log.e(TAG, "writeBytes failed", e)
            false
        }
    }

    // ---- implemented in Rust (src/jni_bridge.rs) ----------------------------
    private external fun nativeOnDocumentOpened(uri: String?, data: ByteArray?)
    private external fun nativeOnModelOpened(uri: String?, data: ByteArray?)
    private external fun nativeOnDocumentCreated(uri: String?)
    private external fun nativeOnSaveComplete(success: Boolean)
    private external fun nativeOnExternalEditReturned(scriptId: Long, text: String?)

    companion object {
        private const val TAG = "rbxl_editor"

        private const val REQ_OPEN = 1001
        private const val REQ_CREATE = 1002

        // Separate request code so a picked local .rbxm/.rbxmx is routed through
        // the model-import path (decoded + inserted into the active place)
        // instead of being treated as a whole .rbxl place file.
        private const val REQ_OPEN_MODEL = 1003

        @Volatile
        private var sInstance: MainActivity? = null

        private var sPlayer: MediaPlayer? = null

        init {
            // Must match the cdylib output name (`rbxl-editor` -> librbxl_editor.so).
            // GameActivity also loads it via the `android.app.lib_name` meta-data;
            // loading twice is a no-op.
            System.loadLibrary("rbxl_editor")
        }

        // ---- called from Rust via JNI (see src/jni_bridge.rs) ---------------

        @JvmStatic
        fun openDocumentStatic() {
            val act = sInstance
            if (act != null) act.openDocument()
            else Log.e(TAG, "openDocumentStatic: MainActivity instance is null")
        }

        /** Launch the system file picker filtered to local model files. */
        @JvmStatic
        fun openModelDocumentStatic() {
            val act = sInstance
            if (act != null) act.openModelDocument()
            else Log.e(TAG, "openModelDocumentStatic: MainActivity instance is null")
        }

        @JvmStatic
        fun createDocumentStatic(suggestedName: String) {
            val act = sInstance
            if (act != null) act.createDocument(suggestedName)
            else Log.e(TAG, "createDocumentStatic: MainActivity instance is null")
        }

        @JvmStatic
        fun saveToCurrentDocumentStatic(data: ByteArray) {
            val act = sInstance
            if (act != null) act.saveToCurrentDocument(data)
            else Log.e(TAG, "saveToCurrentDocumentStatic: MainActivity instance is null")
        }

        @JvmStatic
        fun editExternallyStatic(scriptId: Long, fileName: String, source: String) {
            val act = sInstance
            if (act != null) act.editExternally(scriptId, fileName, source)
            else Log.e(TAG, "editExternallyStatic: MainActivity instance is null")
        }

        @JvmStatic
        fun syncExternalEditsStatic() {
            sInstance?.checkExternalFileUpdate(true)
        }

        @JvmStatic
        fun finishExternalEditStatic() {
            sInstance?.finishExternalEdit()
        }

        /** The app's internal files directory, as an absolute path. */
        @JvmStatic
        fun getFilesDirStatic(): String? {
            val act = sInstance
            if (act == null) {
                Log.e(TAG, "getFilesDirStatic: MainActivity instance is null")
                return null
            }
            return try {
                act.filesDir.absolutePath
            } catch (e: Exception) {
                Log.e(TAG, "getFilesDir failed", e)
                null
            }
        }

        /** Launch the standalone Bevy viewer app by package name. */
        @JvmStatic
        fun launchViewerStatic(packageName: String) {
            val act = sInstance
            if (act == null) {
                Log.e(TAG, "launchViewerStatic: MainActivity instance is null")
                return
            }
            act.launchViewer(packageName)
        }

        @JvmStatic
        fun copyToClipboardStatic(text: String) {
            sInstance?.copyToClipboard(text)
        }

        @JvmStatic
        fun getClipboardTextStatic(): String {
            return sInstance?.getClipboardText() ?: ""
        }

        // ---- Audio playback -------------------------------------------------

        /** Called from Rust via JNI to play a cached ogg/mp3 file. */
        @JvmStatic
        fun playAudioFile(path: String) {
            val act = sInstance ?: return
            act.runOnUiThread {
                try {
                    sPlayer?.release()
                    sPlayer = null
                    val mp = MediaPlayer()
                    mp.setAudioAttributes(
                        AudioAttributes.Builder()
                            .setUsage(AudioAttributes.USAGE_MEDIA)
                            .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
                            .build()
                    )
                    mp.setDataSource(path)
                    mp.setOnPreparedListener { it.start() }
                    mp.setOnErrorListener { _, what, extra ->
                        Log.e(TAG, "MediaPlayer error $what/$extra")
                        Toast.makeText(act, "Audio playback failed", Toast.LENGTH_SHORT).show()
                        true
                    }
                    mp.prepareAsync()
                    sPlayer = mp
                } catch (e: Exception) {
                    Log.e(TAG, "playAudioFile failed: $path", e)
                }
            }
        }

        /** Stop any currently-playing audio. Called from Rust via JNI. */
        @JvmStatic
        fun stopAudio() {
            val act = sInstance ?: return
            act.runOnUiThread {
                val player = sPlayer
                if (player != null) {
                    try {
                        if (player.isPlaying) player.stop()
                        player.release()
                    } catch (ignored: Exception) {
                    }
                    sPlayer = null
                }
            }
        }
    }
}
