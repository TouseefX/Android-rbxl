package com.yourname.rbxleditor;

import android.view.KeyEvent;
import android.view.View;
import android.view.inputmethod.BaseInputConnection;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;

/**
 * A minimal InputConnection that forwards soft-keyboard / Gboard
 * actions to Rust. This is what makes typing and, crucially,
 * long-press paste / clipboard-suggestion paste work in egui's
 * self-rendered text fields on Android: Gboard calls commitText on
 * the InputConnection (rather than reading the system clipboard
 * itself), so we intercept that and push an Event::Paste / Event::Text
 * into the editor.
 */
public class RbxInputConnection extends BaseInputConnection {
    private final View target;

    public RbxInputConnection(View target) {
        super(target, true);
        this.target = target;
    }

    @Override
    public boolean commitText(CharSequence text, int newCursorPosition) {
        if (text == null) return false;
        String s = text.toString();
        if (s.isEmpty()) return true;
        // Gboard's clipboard / long-press Paste also comes through
        // commitText; the editor decides whether to treat a long
        // string as a paste.
        nativeCommitText(s);
        return true;
    }

    @Override
    public boolean deleteSurroundingText(int beforeLength, int afterLength) {
        nativeDeleteSurrounding(beforeLength, afterLength);
        return true;
    }

    @Override
    public boolean sendKeyEvent(KeyEvent event) {
        if (event == null) return false;
        if (event.getAction() == KeyEvent.ACTION_DOWN) {
            nativeKeyDown(event.getKeyCode(), event.getUnicodeChar(), event.isShiftPressed());
        } else {
            nativeKeyUp(event.getKeyCode());
        }
        return true;
    }

    @Override
    public boolean performEditorAction(int actionCode) {
        nativeEditorAction(actionCode);
        return true;
    }

    @Override
    public boolean finishComposingText() {
        nativeFinishComposing();
        return true;
    }

    @Override
    public boolean setComposingText(CharSequence text, int newCursorPosition) {
        // Treat composing text as a normal commit; we don't do IME
        // composition pre-edit rendering.
        if (text != null) nativeCommitText(text.toString());
        return true;
    }

    // ---- Implemented in Rust (src/jni_bridge.rs) ----
    public static native void nativeCommitText(String text);
    public static native void nativeDeleteSurrounding(int before, int after);
    public static native void nativeKeyDown(int keyCode, int unicode, boolean shift);
    public static native void nativeKeyUp(int keyCode);
    public static native void nativeEditorAction(int actionCode);
    public static native void nativeFinishComposing();
}
