package com.yourname.rbxleditor;

import android.app.NativeActivity;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Intent;
import android.net.Uri;
import android.os.Bundle;
import android.os.Environment;
import android.os.StrictMode;
import android.util.Log;
import android.widget.Toast;
import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;

public class MainActivity extends NativeActivity {
    private static final String TAG = "rbxl_editor";
    private static volatile MainActivity sInstance;

    static {
        System.loadLibrary("rbxl_editor"); // must match the cdylib output name
    }

    private static final int REQ_OPEN = 1001;
    private static final int REQ_CREATE = 1002;

    private Uri currentDocUri;

    // External edit state
    private long activeExternalScriptId = -1;
    private String activeExternalFilePath = null;
    private long lastExternalModifiedTime = 0;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        sInstance = this;

        // Allow direct file sharing with external editor apps without FileUriExposedException
        try {
            StrictMode.VmPolicy.Builder builder = new StrictMode.VmPolicy.Builder();
            StrictMode.setVmPolicy(builder.build());
        } catch (Exception e) {
            Log.w(TAG, "StrictMode config exception", e);
        }

        Log.i(TAG, "MainActivity onCreate: instance registered");
    }

    @Override
    protected void onDestroy() {
        if (sInstance == this) {
            sInstance = null;
        }
        super.onDestroy();
    }

    // ---- Static entrypoints called FROM Rust JNI ----

    public static void openDocumentStatic() {
        MainActivity act = sInstance;
        if (act != null) {
            act.openDocument();
        } else {
            Log.e(TAG, "openDocumentStatic: MainActivity instance is null");
        }
    }

    public static void createDocumentStatic(final String suggestedName) {
        MainActivity act = sInstance;
        if (act != null) {
            act.createDocument(suggestedName);
        } else {
            Log.e(TAG, "createDocumentStatic: MainActivity instance is null");
        }
    }

    public static void saveToCurrentDocumentStatic(final byte[] data) {
        MainActivity act = sInstance;
        if (act != null) {
            act.saveToCurrentDocument(data);
        } else {
            Log.e(TAG, "saveToCurrentDocumentStatic: MainActivity instance is null");
        }
    }

    public static void editExternallyStatic(final long scriptId, final String fileName, final String source) {
        MainActivity act = sInstance;
        if (act != null) {
            act.editExternally(scriptId, fileName, source);
        } else {
            Log.e(TAG, "editExternallyStatic: MainActivity instance is null");
        }
    }

    public static void syncExternalEditsStatic() {
        MainActivity act = sInstance;
        if (act != null) {
            act.checkExternalFileUpdate(true);
        }
    }

    public static void finishExternalEditStatic() {
        MainActivity act = sInstance;
        if (act != null) {
            act.finishExternalEdit();
        }
    }

    public static void copyToClipboardStatic(final String text) {
        MainActivity act = sInstance;
        if (act != null) {
            act.copyToClipboard(text);
        }
    }

    public static String getClipboardTextStatic() {
        MainActivity act = sInstance;
        if (act != null) {
            return act.getClipboardText();
        }
        return "";
    }

    // ---- Instance methods running on Android UI Thread ----

    public void copyToClipboard(final String text) {
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                try {
                    ClipboardManager clipboard = (ClipboardManager) getSystemService(CLIPBOARD_SERVICE);
                    if (clipboard != null) {
                        ClipData clip = ClipData.newPlainText("RobloxSnippet", text);
                        clipboard.setPrimaryClip(clip);
                        Toast.makeText(MainActivity.this, "Copied to Android Clipboard!", Toast.LENGTH_SHORT).show();
                    }
                } catch (Exception e) {
                    Log.e(TAG, "copyToClipboard failed", e);
                }
            }
        });
    }

    public String getClipboardText() {
        final String[] result = new String[]{""};
        final CountDownLatch latch = new CountDownLatch(1);
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                try {
                    ClipboardManager clipboard = (ClipboardManager) getSystemService(CLIPBOARD_SERVICE);
                    if (clipboard != null && clipboard.hasPrimaryClip()) {
                        ClipData clip = clipboard.getPrimaryClip();
                        if (clip != null && clip.getItemCount() > 0) {
                            CharSequence text = clip.getItemAt(0).coerceToText(MainActivity.this);
                            if (text != null) {
                                result[0] = text.toString();
                            }
                        }
                    }
                } catch (Exception e) {
                    Log.e(TAG, "getClipboardText failed", e);
                } finally {
                    latch.countDown();
                }
            }
        });

        try {
            latch.await(250, TimeUnit.MILLISECONDS);
        } catch (InterruptedException e) {
            Log.w(TAG, "getClipboardText latch timeout");
        }
        return result[0];
    }

    public void openDocument() {
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                try {
                    Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
                    intent.addCategory(Intent.CATEGORY_OPENABLE);
                    intent.setType("*/*");
                    intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION);
                    startActivityForResult(intent, REQ_OPEN);
                } catch (Exception e) {
                    Log.e(TAG, "openDocument startActivityForResult failed", e);
                    nativeOnDocumentOpened(null, null);
                }
            }
        });
    }

    public void createDocument(final String suggestedName) {
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                try {
                    Intent intent = new Intent(Intent.ACTION_CREATE_DOCUMENT);
                    intent.addCategory(Intent.CATEGORY_OPENABLE);
                    intent.setType("application/octet-stream");
                    intent.putExtra(Intent.EXTRA_TITLE, suggestedName);
                    startActivityForResult(intent, REQ_CREATE);
                } catch (Exception e) {
                    Log.e(TAG, "createDocument failed", e);
                }
            }
        });
    }

    public void saveToCurrentDocument(final byte[] data) {
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                if (currentDocUri == null) {
                    nativeOnSaveComplete(false);
                    return;
                }
                boolean ok = writeBytes(currentDocUri, data);
                nativeOnSaveComplete(ok);
            }
        });
    }

    /**
     * Creates a real .lua script file in Android/media/ (accessible on Android 13+)
     * and hands it directly to external editor apps (QuickEdit, Acode, DroidEdit, etc.)
     * without being blocked by Android 13's Android/data scoped storage restriction.
     */
    public void editExternally(final long scriptId, final String fileName, final String source) {
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                try {
                    // Use Android/media/ directory for full Android 13/14 third-party editor access
                    File scriptsDir = null;
                    File[] mediaDirs = getExternalMediaDirs();
                    if (mediaDirs != null && mediaDirs.length > 0 && mediaDirs[0] != null) {
                        scriptsDir = new File(mediaDirs[0], "scripts");
                    }
                    if (scriptsDir == null || !scriptsDir.exists()) {
                        File mediaRoot = new File(Environment.getExternalStorageDirectory(), "Android/media/com.yourname.rbxleditor/scripts");
                        if (mediaRoot.exists() || mediaRoot.mkdirs()) {
                            scriptsDir = mediaRoot;
                        }
                    }
                    if (scriptsDir == null) {
                        scriptsDir = new File(getExternalFilesDir(null), "scripts");
                    }
                    if (!scriptsDir.exists()) {
                        scriptsDir.mkdirs();
                    }

                    String sanitized = fileName.replaceAll("[^a-zA-Z0-9_.-]", "_");
                    if (!sanitized.endsWith(".lua")) {
                        sanitized = sanitized + ".lua";
                    }

                    File scriptFile = new File(scriptsDir, sanitized);
                    try (FileOutputStream fos = new FileOutputStream(scriptFile)) {
                        fos.write(source.getBytes(StandardCharsets.UTF_8));
                        fos.flush();
                    }

                    activeExternalScriptId = scriptId;
                    activeExternalFilePath = scriptFile.getAbsolutePath();
                    lastExternalModifiedTime = scriptFile.lastModified();

                    Uri fileUri = Uri.fromFile(scriptFile);

                    Intent editIntent = new Intent(Intent.ACTION_VIEW);
                    editIntent.setDataAndType(fileUri, "text/plain");
                    editIntent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION | Intent.FLAG_ACTIVITY_NEW_TASK);

                    Intent chooser = Intent.createChooser(editIntent, "Edit Lua script with...");
                    chooser.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
                    startActivity(chooser);

                    Log.i(TAG, "editExternally: launched chooser in Android/media/ for " + scriptFile.getAbsolutePath());
                } catch (Exception e) {
                    Log.e(TAG, "editExternally failed", e);
                }
            }
        });
    }

    public void checkExternalFileUpdate(boolean force) {
        try {
            if (activeExternalFilePath != null && activeExternalScriptId >= 0) {
                File file = new File(activeExternalFilePath);
                if (file.exists()) {
                    long currentMod = file.lastModified();
                    if (force || currentMod > lastExternalModifiedTime) {
                        lastExternalModifiedTime = currentMod;
                        byte[] bytes = readFile(file);
                        if (bytes != null) {
                            String text = new String(bytes, StandardCharsets.UTF_8);
                            Log.i(TAG, "checkExternalFileUpdate: syncing " + text.length() + " chars from " + file.getName());
                            nativeOnExternalEditReturned(activeExternalScriptId, text);
                        }
                    }
                }
            }
        } catch (Exception e) {
            Log.e(TAG, "checkExternalFileUpdate exception", e);
        }
    }

    public void finishExternalEdit() {
        activeExternalScriptId = -1;
        activeExternalFilePath = null;
        lastExternalModifiedTime = 0;
    }

    @Override
    protected void onResume() {
        super.onResume();
        // Whenever the user switches back to this app, auto-sync any modified external script
        checkExternalFileUpdate(false);
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);

        try {
            if (requestCode == REQ_OPEN) {
                if (resultCode != RESULT_OK || data == null || data.getData() == null) {
                    nativeOnDocumentOpened(null, null);
                    return;
                }
                Uri uri = data.getData();
                persist(uri);
                currentDocUri = uri;
                byte[] bytes = readBytes(uri);
                nativeOnDocumentOpened(uri.toString(), bytes);

            } else if (requestCode == REQ_CREATE) {
                if (resultCode != RESULT_OK || data == null || data.getData() == null) return;
                Uri uri = data.getData();
                persist(uri);
                currentDocUri = uri;
                nativeOnDocumentCreated(uri.toString());
            }
        } catch (Exception e) {
            Log.e(TAG, "onActivityResult exception", e);
        }
    }

    private void persist(Uri uri) {
        try {
            final int flags = Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION;
            getContentResolver().takePersistableUriPermission(uri, flags);
        } catch (Exception e) {
            Log.w(TAG, "takePersistableUriPermission failed (non-fatal)", e);
        }
    }

    private byte[] readBytes(Uri uri) {
        try (InputStream in = getContentResolver().openInputStream(uri)) {
            if (in == null) return null;
            ByteArrayOutputStream buf = new ByteArrayOutputStream();
            byte[] chunk = new byte[8192];
            int n;
            while ((n = in.read(chunk)) != -1) {
                buf.write(chunk, 0, n);
            }
            return buf.toByteArray();
        } catch (Exception e) {
            Log.e(TAG, "readBytes failed", e);
            return null;
        }
    }

    private byte[] readFile(File file) {
        try (FileInputStream in = new FileInputStream(file)) {
            ByteArrayOutputStream buf = new ByteArrayOutputStream();
            byte[] chunk = new byte[8192];
            int n;
            while ((n = in.read(chunk)) != -1) {
                buf.write(chunk, 0, n);
            }
            return buf.toByteArray();
        } catch (Exception e) {
            Log.e(TAG, "readFile failed: " + file, e);
            return null;
        }
    }

    private boolean writeBytes(Uri uri, byte[] data) {
        try (OutputStream out = getContentResolver().openOutputStream(uri, "wt")) {
            if (out == null) return false;
            out.write(data);
            out.flush();
            return true;
        } catch (Exception e) {
            Log.e(TAG, "writeBytes failed", e);
            return false;
        }
    }

    // ---- implemented in Rust (src/jni_bridge.rs) ----
    private native void nativeOnDocumentOpened(String uri, byte[] data);
    private native void nativeOnDocumentCreated(String uri);
    private native void nativeOnSaveComplete(boolean success);
    private native void nativeOnExternalEditReturned(long scriptId, String text);
}
