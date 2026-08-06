package com.yourname.rbxleditor;

import android.app.NativeActivity;
import android.content.Intent;
import android.net.Uri;
import android.os.Bundle;
import android.util.Log;
import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;

public class MainActivity extends NativeActivity {
    private static final String TAG = "rbxl_editor";
    private static volatile MainActivity sInstance;

    static {
        System.loadLibrary("rbxl_editor"); // must match the cdylib output name
    }

    private static final int REQ_OPEN = 1001;
    private static final int REQ_CREATE = 1002;
    private static final int REQ_EXTERNAL_EDIT_CREATE = 1003;

    private Uri currentDocUri;      // the .rbxl file itself
    private Uri externalEditUri;    // temp .lua doc handed to another app
    private long externalEditScriptId = -1;
    private String pendingExternalSource;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        sInstance = this;
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

    // ---- Instance methods running on Android UI Thread ----

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

    public void editExternally(final long scriptId, final String fileName, final String source) {
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                try {
                    externalEditScriptId = scriptId;
                    pendingExternalSource = source;
                    Intent create = new Intent(Intent.ACTION_CREATE_DOCUMENT);
                    create.addCategory(Intent.CATEGORY_OPENABLE);
                    create.setType("text/plain");
                    create.putExtra(Intent.EXTRA_TITLE, fileName);
                    startActivityForResult(create, REQ_EXTERNAL_EDIT_CREATE);
                } catch (Exception e) {
                    Log.e(TAG, "editExternally failed", e);
                }
            }
        });
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

            } else if (requestCode == REQ_EXTERNAL_EDIT_CREATE) {
                if (resultCode != RESULT_OK || data == null || data.getData() == null) return;
                externalEditUri = data.getData();
                persist(externalEditUri);
                if (pendingExternalSource != null) {
                    writeBytes(externalEditUri, pendingExternalSource.getBytes(StandardCharsets.UTF_8));
                    pendingExternalSource = null;
                }

                Intent view = new Intent(Intent.ACTION_VIEW);
                view.setDataAndType(externalEditUri, "text/plain");
                view.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION);
                startActivity(Intent.createChooser(view, "Edit script with..."));
            }
        } catch (Exception e) {
            Log.e(TAG, "onActivityResult exception", e);
        }
    }

    @Override
    protected void onResume() {
        super.onResume();
        // If we sent a script out for external editing, read it back now.
        try {
            if (externalEditUri != null && externalEditScriptId >= 0) {
                byte[] bytes = readBytes(externalEditUri);
                String text = bytes != null ? new String(bytes, StandardCharsets.UTF_8) : "";
                nativeOnExternalEditReturned(externalEditScriptId, text);
                externalEditUri = null;
                externalEditScriptId = -1;
            }
        } catch (Exception e) {
            Log.e(TAG, "onResume exception", e);
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
