package com.yourname.rbxleditor

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.app.Dialog
import android.graphics.Color
import android.graphics.Typeface
import android.media.AudioAttributes
import android.media.MediaPlayer
import android.net.Uri
import android.os.Bundle
import android.os.Environment
import android.os.Looper
import android.os.StrictMode
import android.util.Log
import android.view.Gravity
import android.view.View
import android.view.WindowManager
import android.view.inputmethod.InputMethodManager
import android.widget.Button
import android.widget.LinearLayout
import android.widget.HorizontalScrollView
import android.widget.TextView
import android.widget.Toast
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import androidx.core.view.ViewCompat
import com.google.androidgamesdk.GameActivity
import io.github.rosemoe.sora.event.ContentChangeEvent
import io.github.rosemoe.sora.widget.CodeEditor
import io.github.rosemoe.sora.widget.component.EditorAutoCompletion
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream
import java.nio.charset.StandardCharsets
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import org.json.JSONObject
import org.json.JSONArray

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
    private var activeProjectRoot: File? = null
    private val projectModifiedTimes = HashMap<String, Long>()
    private var nativeEditorDialog: Dialog? = null
    private var nativeEditorScriptId: Long = -1
    /** Native editor line-wrap preference; off keeps code on one visual line. */
    private var nativeEditorWordWrap: Boolean = false

    /** The script editor and its language, non-null only while one is open. */
    private var soraEditorView: CodeEditor? = null
    private var soraLanguage: LuauLanguage? = null

    /** Last sora construction failure, shown on screen when adb is unavailable. */
    private var lastSoraError: String? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        // Register the instance BEFORE super.onCreate(): GameActivity's
        // onCreate() loads the native library and spawns the `android_main`
        // thread, which immediately starts loading persisted settings/plugins
        // through JNI (getFilesDirStatic). If sInstance were only set after
        // super.onCreate() returns, that very first native load would race the
        // UI thread, see a null instance, and fall back to an unwritable
        // path — so saved settings never came back after a restart.
        sInstance = this

        // Let Android resize the native surface above the Samsung/Gboard IME.
        // Edge-to-edge (`false`) prevents adjustResize on several One UI builds
        // and leaves the editor hidden behind the keyboard.
        WindowCompat.setDecorFitsSystemWindows(window, true)
        hideSystemUi()
        super.onCreate(savedInstanceState)
        installImeResizeHandling()

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
        syncExportedProject()
    }

    /**
     * GameActivity owns a native SurfaceView, and on some Samsung One UI
     * versions the normal adjustResize flag does not resize that surface.
     * Apply the IME inset to the content FrameLayout explicitly so Bevy's
     * drawable area ends above the keyboard instead of being covered by it.
     */
    private fun installImeResizeHandling() {
        window.setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_NOTHING)
        val content = findViewById<View>(android.R.id.content) ?: window.decorView
        ViewCompat.setOnApplyWindowInsetsListener(content) { view, insets ->
            val ime = insets.getInsets(WindowInsetsCompat.Type.ime())
            val keyboardVisible = insets.isVisible(WindowInsetsCompat.Type.ime())
            imeVisible = keyboardVisible
            val bottom = if (keyboardVisible) ime.bottom else 0
            if (view.paddingBottom != bottom) {
                view.setPadding(view.paddingLeft, view.paddingTop, view.paddingRight, bottom)
                view.requestLayout()
            }
            insets
        }
        ViewCompat.requestApplyInsets(content)
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

    /** Export a complete Rojo-style project where Android code editors can open it. */
    fun exportProject(bundleJson: String) {
        runOnUiThread {
            try {
                val bundle = JSONObject(bundleJson)
                val name = bundle.optString("name", "RobloxProject")
                    .replace(Regex("[^a-zA-Z0-9_.-]"), "_")
                val media = externalMediaDirs.firstOrNull()
                    ?: File(Environment.getExternalStorageDirectory(), "Android/media/$packageName")
                val root = File(media, "projects/$name")
                if (!root.exists()) root.mkdirs()
                val files = bundle.getJSONArray("files")
                for (i in 0 until files.length()) {
                    val item = files.getJSONObject(i)
                    val relative = item.getString("path")
                    val target = File(root, relative)
                    // Reject traversal even if malformed project data reaches JNI.
                    if (!target.canonicalPath.startsWith(root.canonicalPath + File.separator)) continue
                    target.parentFile?.mkdirs()
                    target.writeText(item.getString("content"), StandardCharsets.UTF_8)
                    projectModifiedTimes[target.canonicalPath] = target.lastModified()
                }
                activeProjectRoot = root
                Toast.makeText(this, "Project exported: ${root.absolutePath}", Toast.LENGTH_LONG).show()
                Log.i(TAG, "exportProject: ${files.length()} files to ${root.absolutePath}")
            } catch (e: Exception) {
                Log.e(TAG, "exportProject failed", e)
                Toast.makeText(this, "Project export failed: ${e.message}", Toast.LENGTH_LONG).show()
            }
        }
    }

    /** Import changed script files when returning from an external project editor. */
    private fun syncExportedProject() {
        val root = activeProjectRoot ?: return
        try {
            val files = JSONArray()
            File(root, "src").walkTopDown().filter { it.isFile && it.extension == "luau" }.forEach { file ->
                val key = file.canonicalPath
                val modified = file.lastModified()
                if (modified > (projectModifiedTimes[key] ?: 0L)) {
                    files.put(JSONObject().apply {
                        put("path", file.relativeTo(root).invariantSeparatorsPath)
                        put("content", file.readText(StandardCharsets.UTF_8))
                    })
                    projectModifiedTimes[key] = modified
                }
            }
            if (files.length() > 0) nativeOnProjectSync(JSONObject().put("files", files).toString())
        } catch (e: Exception) {
            Log.e(TAG, "syncExportedProject failed", e)
        }
    }

    /**
     * Writes a real .luau file into Android/media/ (which stays reachable by
     * third-party editors on Android 13+, unlike Android/data) and hands it to
     * whatever editor app the user picks. Luau-aware editors use the extension
     * for the correct grammar while text/plain keeps broad Android app support.
     */





    /**
     * Full-screen native Luau editor, built on sora-editor. Unlike the Bevy
     * SurfaceView this is a real Android view, so the platform owns caret
     * placement, kinetic scrolling, selection handles and the system
     * Copy/Cut/Paste ActionMode.
     */
    fun showNativeEditor(scriptId: Long, fileName: String, source: String, initialCursor: Int = 0) {
        // Built defensively: anything thrown while constructing the editor
        // would otherwise take the whole process down with no way to see why
        // on a device without adb, so the exception is surfaced on screen
        // instead.
        //
        // The guard must run *inside* runOnUiThread. This method is called
        // from the Rust thread through JNI, so a try/catch out here would
        // return before the posted work ever executes and would catch nothing.
        runOnUiThread {
            try {
                showSoraEditor(scriptId, fileName, source, initialCursor)
            } catch (error: Throwable) {
                Log.e(TAG, "sora editor failed to open", error)
                lastSoraError = describeThrowable(error)
                nativeEditorDialog?.dismiss()
                nativeEditorDialog = null
                soraEditorView = null
                soraLanguage = null
                showSoraErrorDialog()
            }
        }
    }

    /** Exception text and top frames, formatted for on-screen reporting. */
    private fun describeThrowable(error: Throwable): String {
        val text = StringBuilder()
        var current: Throwable? = error
        var depth = 0
        while (current != null && depth < 4) {
            if (depth > 0) text.append("\nCaused by: ")
            text.append(current.javaClass.name)
            current.message?.let { text.append(": ").append(it) }
            current.stackTrace.take(6).forEach { text.append("\n    at ").append(it) }
            current = current.cause
            depth++
        }
        return text.toString()
    }

    /**
     * Shows why the editor could not open, with the trace selectable and
     * copyable so it can be reported off a device that has no adb.
     */
    private fun showSoraErrorDialog() {
        val dialog = Dialog(this, android.R.style.Theme_Material_NoActionBar_Fullscreen)
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(Color.rgb(30, 30, 30))
            setPadding(20, 20, 20, 20)
        }
        root.addView(TextView(this).apply {
            text = "The new editor failed to open"
            setTextColor(Color.WHITE)
            textSize = 18f
            typeface = Typeface.DEFAULT_BOLD
        })
        val trace = TextView(this).apply {
            text = lastSoraError ?: "(no detail captured)"
            setTextColor(Color.rgb(255, 170, 170))
            typeface = Typeface.MONOSPACE
            textSize = 11f
            setTextIsSelectable(true)
        }
        root.addView(android.widget.ScrollView(this).apply { addView(trace) },
            LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, 0, 1f))

        val buttons = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        buttons.addView(Button(this).apply {
            text = "Copy"
            setOnClickListener {
                val clip = getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
                clip?.setPrimaryClip(ClipData.newPlainText("sora error", lastSoraError ?: ""))
                Toast.makeText(this@MainActivity, "Copied", Toast.LENGTH_SHORT).show()
            }
        })
        buttons.addView(Button(this).apply {
            text = "Close"
            setOnClickListener {
                dialog.dismiss()
                nativeEditorDialog = null
            }
        })
        root.addView(buttons)
        dialog.setContentView(root)
        dialog.show()
        nativeEditorDialog = dialog
    }

    /**
     * Script editor built on sora-editor: the gutter, syntax highlighting,
     * bracket pairing, smart Enter and the completion window all come from
     * the library, so the only wiring left here is the toolbar, the symbol
     * row and the Rust intelligence bridge.
     */
    private fun showSoraEditor(scriptId: Long, fileName: String, source: String, initialCursor: Int) {
        // Runs on the UI thread already: the caller wraps this in
        // runOnUiThread so construction failures are catchable.
        nativeEditorDialog?.dismiss()

        val dialog = Dialog(this, android.R.style.Theme_Material_NoActionBar_Fullscreen)
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(Color.rgb(30, 30, 30))
        }
        val toolbar = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(12, 8, 12, 8)
            setBackgroundColor(Color.rgb(42, 42, 44))
        }
        val title = TextView(this).apply {
            text = fileName
            setTextColor(Color.WHITE)
            textSize = 16f
            typeface = Typeface.DEFAULT_BOLD
        }
        toolbar.addView(title, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
        val cancel = Button(this).apply { text = "Cancel" }
        val done = Button(this).apply { text = "Done" }
        toolbar.addView(cancel)
        toolbar.addView(done)
        root.addView(toolbar, LinearLayout.LayoutParams(
            LinearLayout.LayoutParams.MATCH_PARENT,
            LinearLayout.LayoutParams.WRAP_CONTENT
        ))

        val language = LuauLanguage(scriptId) { id, text, cursor ->
            // Called on sora's completion worker; the JNI bridge is
            // thread-safe and the reply comes back through
            // updateNativeCompletions -> deliverCompletions.
            nativeOnNativeEditorChanged(id, text, cursor, cursor)
        }

        // Tabs already stored in the script become spaces so on-screen
        // columns match what the indent logic produces.
        val normalizedSource = source.replace("\t", INDENT_UNIT)
        val editor = CodeEditor(this).apply {
            setEditorLanguage(language)
            colorScheme = LuauLanguage.darkScheme()
            typefaceText = Typeface.MONOSPACE
            setTextSize(15f)
            setLineNumberEnabled(true)
            setPinLineNumber(true)
            setWordwrap(nativeEditorWordWrap)
            tabWidth = INDENT_WIDTH
            isHighlightCurrentLine = true
            // Two completion requests closer together than this are dropped
            // and the window is hidden. The 70ms default assumes a language
            // that answers instantly; ours makes a round trip to the Rust
            // index, so ordinary typing kept landing inside the window and
            // the popup only appeared after a pause -- which is why typing a
            // space and deleting it "fixed" it. Effectively disabled.
            getProps().cancelCompletionNs = 0L
            // Backspace removes a whole indent level (tabWidth spaces) at
            // once when the caret sits in a line's leading whitespace, the
            // way PC editors treat tab stops. sora's default removes one
            // space per press, so a 4-space indent took 4 backspaces.
            // -1 means "follow tab size"; only leading whitespace is
            // affected, deletes elsewhere still remove a single character.
            getProps().deleteMultiSpaces = -1
            setText(normalizedSource)
        }
        // Re-ask for completions after an auto-paired quote or bracket.
        //
        // commitText() inserts the pair and then calls setSelection() with
        // CAUSE_UNKNOWN, and EditorAutoCompletion.onSelectionChange() hides the
        // window unconditionally for that cause. So typing the opening quote of
        // require('') showed nothing, while typing a '.' afterwards -- a plain
        // insert with no pairing -- worked. Re-requesting on the next frame
        // runs after the hide() and puts the path list back.
        editor.subscribeEvent(ContentChangeEvent::class.java) { event, _ ->
            if (event.action == ContentChangeEvent.ACTION_INSERT &&
                event.changedText.length == 1 &&
                event.changedText[0] in PAIRED_OPENERS
            ) {
                editor.post {
                    editor.getComponent(EditorAutoCompletion::class.java).requireCompletion()
                }
            }
        }

        // Position the caret by translating the Rust character index into
        // the (line, column) pair sora addresses text with.
        val caret = editor.text.getIndexer().getCharPosition(
            initialCursor.coerceIn(0, normalizedSource.length)
        )
        editor.setSelection(caret.line, caret.column)

        val actions = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(10, 2, 10, 2)
            setBackgroundColor(Color.rgb(36, 36, 38))
        }
        val checkLuau = Button(this).apply { text = "✓ Check" }
        val formatLuau = Button(this).apply { text = "✨ Format" }
        val goDefinition = Button(this).apply { text = "↗ Definition" }
        val findReferences = Button(this).apply { text = "⌕ References" }
        val toggleWrap = Button(this).apply {
            text = if (nativeEditorWordWrap) "↩ Wrap: On" else "↩ Wrap: Off"
        }
        actions.addView(checkLuau)
        actions.addView(formatLuau)
        actions.addView(toggleWrap)
        actions.addView(goDefinition)
        actions.addView(findReferences)
        root.addView(HorizontalScrollView(this).apply {
            isHorizontalScrollBarEnabled = false
            addView(actions)
        }, LinearLayout.LayoutParams(
            LinearLayout.LayoutParams.MATCH_PARENT,
            LinearLayout.LayoutParams.WRAP_CONTENT
        ))

        fun caretIndex(): Int = editor.cursor.left

        checkLuau.setOnClickListener {
            nativeOnNativeEditorCommand(scriptId, "check", editor.text.toString(), caretIndex())
        }
        formatLuau.setOnClickListener {
            nativeOnNativeEditorCommand(scriptId, "format", editor.text.toString(), caretIndex())
        }
        goDefinition.setOnClickListener {
            nativeOnNativeEditorCommand(scriptId, "definition", editor.text.toString(), caretIndex())
        }
        findReferences.setOnClickListener {
            nativeOnNativeEditorCommand(scriptId, "references", editor.text.toString(), caretIndex())
        }
        toggleWrap.setOnClickListener {
            nativeEditorWordWrap = !nativeEditorWordWrap
            toggleWrap.text = if (nativeEditorWordWrap) "↩ Wrap: On" else "↩ Wrap: Off"
            editor.setWordwrap(nativeEditorWordWrap)
        }

        root.addView(editor, LinearLayout.LayoutParams(
            LinearLayout.LayoutParams.MATCH_PARENT, 0, 1f
        ))

        // Symbol row for characters Android keyboards bury in submenus.
        val symbolRow = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            setPadding(6, 2, 6, 2)
            setBackgroundColor(Color.rgb(42, 42, 44))
        }
        for (symbol in EXTRA_KEYS) {
            val key = Button(this).apply {
                text = if (symbol == "\t") "⇥" else symbol
                textSize = 15f
                minWidth = 0
                minimumWidth = 0
                setPadding(20, 4, 20, 4)
                setOnClickListener {
                    // insertText replaces the selection and moves the caret.
                    // The offset must be the full length so the caret lands
                    // after the inserted text, not one character into it.
                    if (symbol == "\t") {
                        // Like a PC Tab key: when several lines are selected,
                        // indent every line of the block instead of replacing
                        // the selection. (Shift+Tab / outdent is the ⇤ key.)
                        val cursor = editor.cursor
                        if (cursor.isSelected && cursor.leftLine != cursor.rightLine) {
                            editor.indentSelection()
                        } else {
                            editor.insertText(INDENT_UNIT, INDENT_UNIT.length)
                        }
                    } else {
                        editor.insertText(symbol, symbol.length)
                    }
                    editor.requestFocus()
                }
            }
            symbolRow.addView(key)
            if (symbol == "\t") {
                // Shift+Tab equivalent for touch: removes one indent level
                // from every selected line (sora falls back to the caret's
                // line when nothing is selected).
                val outdent = Button(this).apply {
                    text = "⇤"
                    textSize = 15f
                    minWidth = 0
                    minimumWidth = 0
                    setPadding(20, 4, 20, 4)
                    setOnClickListener {
                        editor.unindentSelection()
                        editor.requestFocus()
                    }
                }
                symbolRow.addView(outdent)
            }
        }
        root.addView(HorizontalScrollView(this).apply {
            isHorizontalScrollBarEnabled = false
            addView(symbolRow)
        }, LinearLayout.LayoutParams(
            LinearLayout.LayoutParams.MATCH_PARENT,
            LinearLayout.LayoutParams.WRAP_CONTENT
        ))

        fun close(apply: Boolean) {
            if (apply) nativeOnExternalEditReturned(scriptId, editor.text.toString())
            val imm = getSystemService(Context.INPUT_METHOD_SERVICE) as? InputMethodManager
            imm?.hideSoftInputFromWindow(editor.windowToken, 0)
            soraEditorView = null
            soraLanguage = null
            nativeEditorScriptId = -1
            // Frees the editor's threads and the language's analyzer.
            editor.release()
            dialog.dismiss()
            nativeEditorDialog = null
        }
        cancel.setOnClickListener { close(false) }
        done.setOnClickListener { close(true) }
        dialog.setOnCancelListener { nativeEditorDialog = null }
        dialog.setContentView(root)
        dialog.window?.setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE)
        dialog.show()
        nativeEditorDialog = dialog
        soraEditorView = editor
        soraLanguage = language
        nativeEditorScriptId = scriptId
        editor.requestFocus()
    }


    fun updateNativeEditorResult(scriptId: Long, command: String, text: String, message: String) {
        runOnUiThread {
            val editor = soraEditorView ?: return@runOnUiThread
            if (nativeEditorScriptId != scriptId) return@runOnUiThread
            if (command == "format" && editor.text.toString() != text) {
                val cursor = editor.cursor.left
                editor.setText(text)
                val position = editor.text.getIndexer()
                    .getCharPosition(cursor.coerceIn(0, text.length))
                editor.setSelection(position.line, position.column)
            }
            if (message.isNotBlank()) {
                Toast.makeText(this, message, Toast.LENGTH_LONG).show()
            }
            editor.requestFocus()
        }
    }

    fun updateNativeCompletions(scriptId: Long, json: String) {
        runOnUiThread {
            // Hand straight to the language, which unparks the worker blocked
            // in requireAutoComplete. Sora owns the popup, so there is no
            // ListPopupWindow to build, position or dismiss here.
            val language = soraLanguage ?: return@runOnUiThread
            if (nativeEditorScriptId != scriptId) return@runOnUiThread
            language.deliverCompletions(json)
        }
    }

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
                // Roblox scripts use Luau. Replace a legacy extension rather
                // than producing names like Foo.lua.luau.
                sanitized = when {
                    sanitized.endsWith(".luau", ignoreCase = true) -> sanitized
                    sanitized.endsWith(".lua", ignoreCase = true) -> sanitized.dropLast(4) + ".luau"
                    else -> "$sanitized.luau"
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

                val chooser = Intent.createChooser(editIntent, "Edit Luau script with...")
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
    private external fun nativeOnNativeEditorChanged(scriptId: Long, text: String?, selectionStart: Int, selectionEnd: Int)
    private external fun nativeOnNativeEditorCommand(scriptId: Long, command: String?, text: String?, cursor: Int)
    private external fun nativeOnProjectSync(bundleJson: String)

    companion object {
        private const val TAG = "rbxl_editor"

        /**
         * Indentation width in spaces.
         *
         * Spaces rather than '\t', matching LuauLanguage.INDENT_WIDTH and
         * app::INDENT_WIDTH on the Rust side so a column means the same
         * thing everywhere.
         */
        const val INDENT_WIDTH = 4
        val INDENT_UNIT = " ".repeat(INDENT_WIDTH)

        /**
         * Openers that LuauLanguage auto-pairs. Inserting one ends with a
         * setSelection(CAUSE_UNKNOWN) that hides the completion window, so
         * these are the characters after which it has to be re-requested.
         */
        val PAIRED_OPENERS = charArrayOf('"', '\'', '(', '[', '{')

        /** Quick-insert symbol row for keys Android keyboards bury in submenus. */
        val EXTRA_KEYS = listOf(
            "\t", "(", ")", "\"", "{", "}", "[", "]", ";", ":", ",", ".", "=", "-", "_", "#"
        )

        private const val REQ_OPEN = 1001
        private const val REQ_CREATE = 1002

        // Separate request code so a picked local .rbxm/.rbxmx is routed through
        // the model-import path (decoded + inserted into the active place)
        // instead of being treated as a whole .rbxl place file.
        private const val REQ_OPEN_MODEL = 1003

        @Volatile
        private var sInstance: MainActivity? = null

        /** Updated from WindowInsets, including when Back dismisses the IME. */
        @Volatile
        private var imeVisible: Boolean = false

        private var sPlayer: MediaPlayer? = null

        init {
            // Must match the cdylib output name (`rbxl-editor` -> librbxl_editor.so).
            // GameActivity also loads it via the `android.app.lib_name` meta-data;
            // loading twice is a no-op.
            System.loadLibrary("rbxl_editor")
        }

        // ---- called from Rust via JNI (see src/jni_bridge.rs) ---------------

        @JvmStatic
        fun isImeVisibleStatic(): Boolean = imeVisible

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
        fun exportProjectStatic(bundleJson: String) {
            val act = sInstance
            if (act != null) act.exportProject(bundleJson)
            else Log.e(TAG, "exportProjectStatic: MainActivity instance is null")
        }

        @JvmStatic
        fun showNativeEditorStatic(scriptId: Long, fileName: String, source: String) {
            val act = sInstance
            if (act != null) act.showNativeEditor(scriptId, fileName, source, 0)
            else Log.e(TAG, "showNativeEditorStatic: MainActivity instance is null")
        }

        @JvmStatic
        fun showNativeEditorAtStatic(scriptId: Long, fileName: String, source: String, cursor: Int) {
            val act = sInstance
            if (act != null) act.showNativeEditor(scriptId, fileName, source, cursor)
            else Log.e(TAG, "showNativeEditorAtStatic: MainActivity instance is null")
        }

        @JvmStatic
        fun updateNativeEditorResultStatic(scriptId: Long, command: String, text: String, message: String) {
            sInstance?.updateNativeEditorResult(scriptId, command, text, message)
        }

        @JvmStatic
        fun updateNativeCompletionsStatic(scriptId: Long, json: String) {
            sInstance?.updateNativeCompletions(scriptId, json)
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
