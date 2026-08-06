package com.yourname.rbxleditor;

import android.app.NativeActivity;
import android.content.Intent;
import android.net.Uri;
import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.io.OutputStream;

public class MainActivity extends NativeActivity {
    static {
        System.loadLibrary("rbxl_editor"); // must match the cdylib output name
    }

    private static final int REQ_OPEN = 1001;
    private static final int REQ_CREATE = 1002;
    private static final int REQ_EXTERNAL_EDIT_CREATE = 1003;

    private Uri currentDocUri;      // the .rbxl file itself
    private Uri externalEditUri;    // temp .lua doc handed to another app
    private long externalEditScriptId = -1;

    // ---- called FROM Rust ----

    public void openDocument() {
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
        intent.addCategory(Intent.CATEGORY_OPENABLE);
        intent.setType("*/*"); // .rbxl has no registered MIME type
        startActivityForResult(intent, REQ_OPEN);
    }

    public void createDocument(String suggestedName) {
        Intent intent = new Intent(Intent.ACTION_CREATE_DOCUMENT);
        intent.addCategory(Intent.CATEGORY_OPENABLE);
        intent.setType("application/octet-stream");
        intent.putExtra(Intent.EXTRA_TITLE, suggestedName);
        startActivityForResult(intent, REQ_CREATE);
    }

    public void saveToCurrentDocument(byte[] data) {
        if (currentDocUri == null) return;
        boolean ok = writeBytes(currentDocUri, data);
        nativeOnSaveComplete(ok);
    }

    /**
     * Writes `source` into a brand-new SAF document (so it has a real,
     * shareable content:// URI) and hands it to whatever text editor app
     * the user picks. We get no callback when they're done — instead we
     * re-read this file in onResume() when the user comes back to us.
     */
    public void editExternally(long scriptId, String fileName, String source) {
        externalEditScriptId = scriptId;
        Intent create = new Intent(Intent.ACTION_CREATE_DOCUMENT);
        create.addCategory(Intent.CATEGORY_OPENABLE);
        create.setType("text/plain");
        create.putExtra(Intent.EXTRA_TITLE, fileName);
        // stash the source in the instance field; written once we get the URI back
        pendingExternalSource = source;
        startActivityForResult(create, REQ_EXTERNAL_EDIT_CREATE);
    }

    private String pendingExternalSource;

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);

        if (requestCode == REQ_OPEN) {
            if (resultCode != RESULT_OK || data == null || data.getData() == null) {
                nativeOnDocumentOpened(null, null);
                return;
            }
            Uri uri = data.getData();
            persist(uri);
            currentDocUri = uri;
            nativeOnDocumentOpened(uri.toString(), readBytes(uri));

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
            writeBytes(externalEditUri, pendingExternalSource.getBytes());
            pendingExternalSource = null;

            Intent view = new Intent(Intent.ACTION_VIEW);
            view.setDataAndType(externalEditUri, "text/plain");
            view.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION);
            startActivity(Intent.createChooser(view, "Edit script with..."));
        }
    }

    @Override
    protected void onResume() {
        super.onResume();
        // If we sent a script out for external editing, read it back now.
        if (externalEditUri != null && externalEditScriptId >= 0) {
            byte[] bytes = readBytes(externalEditUri);
            String text = bytes != null ? new String(bytes) : "";
            nativeOnExternalEditReturned(externalEditScriptId, text);
            externalEditUri = null;
            externalEditScriptId = -1;
        }
    }

    private void persist(Uri uri) {
        getContentResolver().takePersistableUriPermission(
            uri,
            Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION
        );
    }

    private byte[] readBytes(Uri uri) {
        try (InputStream in = getContentResolver().openInputStream(uri)) {
            ByteArrayOutputStream buf = new ByteArrayOutputStream();
            byte[] chunk = new byte[8192];
            int n;
            while ((n = in.read(chunk)) != -1) buf.write(chunk, 0, n);
            return buf.toByteArray();
        } catch (Exception e) {
            return null;
        }
    }

    private boolean writeBytes(Uri uri, byte[] data) {
        try (OutputStream out = getContentResolver().openOutputStream(uri, "wt")) {
            out.write(data);
            return true;
        } catch (Exception e) {
            return false;
        }
    }

    // ---- implemented in Rust (src/jni_bridge.rs) ----
    private native void nativeOnDocumentOpened(String uri, byte[] data);
    private native void nativeOnDocumentCreated(String uri);
    private native void nativeOnSaveComplete(boolean success);
    private native void nativeOnExternalEditReturned(long scriptId, String text);
}
