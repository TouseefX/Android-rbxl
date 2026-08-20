package com.yourname.rbxleditor;

import android.content.Context;
import android.text.InputType;
import android.view.KeyEvent;
import android.view.View;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputConnectionWrapper;
import android.view.inputmethod.InputMethodManager;
import android.widget.EditText;

/**
 * Invisible EditText whose InputConnection forwards soft-keyboard
 * events (typing, composing, Gboard/Samsung Keyboard paste) to Rust.
 *
 * It is sized 1x1 and parked off-screen so it never draws or
 * intercepts touches, but it is a real editable view — IMEs
 * (including Samsung Keyboard) reliably bind to it.
 *
 * We keep a small in-memory "composition buffer" (the text the IME
 * is currently composing via setComposingText) so methods like
 * getTextBeforeCursor return what the keyboard expects to see; we
 * commit text to Rust when the IME finalizes it.
 */
public class ImeBridge extends EditText {
    public ImeBridge(Context context) {
        super(context);
        setBackgroundColor(0x00000000);
        setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_MULTI_LINE);
        setImeOptions(EditorInfo.IME_ACTION_DONE);
        setWidth(1);
        setHeight(1);
        setVisibility(View.VISIBLE);
        setFocusable(true);
        setFocusableInTouchMode(true);
        setClickable(false);
        setLongClickable(false);
        setCursorVisible(false);
    }

    @Override
    public boolean onCheckIsTextEditor() {
        return hasFocus();
    }

    @Override
    public InputConnection onCreateInputConnection(EditorInfo outAttrs) {
        InputConnection base = super.onCreateInputConnection(outAttrs);
        if (base == null) return null;

        // The wrapping EditText holds a composing buffer we read from;
        // we just never display it.
        return new InputConnectionWrapper(base, false) {
            // ---- Text commit (typing finalization + paste) ----
            @Override
            public boolean commitText(CharSequence text, int newCursorPosition) {
                if (text != null) {
                    String s = text.toString();
                    if (!s.isEmpty()) nativeCommitText(s);
                }
                return super.commitText(text, newCursorPosition);
            }

            // ---- Composing text (Samsung Keyboard word suggestions) ----
            @Override
            public boolean setComposingText(CharSequence text, int newCursorPosition) {
                // Let the wrapped EditText track the composing span so
                // its getTextBeforeCursor/getSelectedText return
                // plausible values; we don't push in-progress composing
                // text to egui (it would add every prediction
                // character). Final text arrives via commitText.
                return super.setComposingText(text, newCursorPosition);
            }

            @Override
            public boolean finishComposingText() {
                nativeFinishComposing();
                return super.finishComposingText();
            }

            // ---- Deletion ----
            @Override
            public boolean deleteSurroundingText(int beforeLength, int afterLength) {
                if (beforeLength > 0) nativeDeleteSurrounding(beforeLength, afterLength);
                return super.deleteSurroundingText(beforeLength, afterLength);
            }

            @Override
            public boolean sendKeyEvent(KeyEvent event) {
                // Samsung Keyboard sends backspace/enter as key events
                // on some configurations.
                if (event.getAction() == KeyEvent.ACTION_DOWN) {
                    nativeKeyDown(event.getKeyCode(), event.getUnicodeChar(), event.isShiftPressed());
                } else {
                    nativeKeyUp(event.getKeyCode());
                }
                return super.sendKeyEvent(event);
            }

            @Override
            public boolean performEditorAction(int actionCode) {
                nativeEditorAction(actionCode);
                return super.performEditorAction(actionCode);
            }

            // ---- Read methods: return the wrapped EditText's buffer ----
            // Samsung Keyboard queries these during composing; the
            // default InputConnectionWrapper routes them to the
            // EditText, which maintains a small invisible buffer.
            // We keep them explicitly for clarity and to return
            // non-null empty strings instead of null on older
            // devices.
            @Override
            public CharSequence getTextBeforeCursor(int length, int flags) {
                CharSequence s = super.getTextBeforeCursor(length, flags);
                return s == null ? "" : s;
            }

            @Override
            public CharSequence getTextAfterCursor(int length, int flags) {
                CharSequence s = super.getTextAfterCursor(length, flags);
                return s == null ? "" : s;
            }

            @Override
            public CharSequence getSelectedText(int flags) {
                CharSequence s = super.getSelectedText(flags);
                return s == null ? "" : s;
            }
        };
    }

    public void showKeyboard() {
        post(() -> {
            requestFocus();
            InputMethodManager imm = (InputMethodManager)
                    getContext().getSystemService(Context.INPUT_METHOD_SERVICE);
            if (imm != null) {
                imm.showSoftInput(this, InputMethodManager.SHOW_IMPLICIT);
            }
        });
    }

    public void hideKeyboard() {
        post(() -> {
            InputMethodManager imm = (InputMethodManager)
                    getContext().getSystemService(Context.INPUT_METHOD_SERVICE);
            if (imm != null && getWindowToken() != null) {
                imm.hideSoftInputFromWindow(getWindowToken(), 0);
            }
            clearFocus();
        });
    }

    // ---- Implemented in Rust (src/jni_bridge.rs) ----
    public static native void nativeCommitText(String text);
    public static native void nativeDeleteSurrounding(int before, int after);
    public static native void nativeKeyDown(int keyCode, int unicode, boolean shift);
    public static native void nativeKeyUp(int keyCode);
    public static native void nativeEditorAction(int actionCode);
    public static native void nativeFinishComposing();
}
