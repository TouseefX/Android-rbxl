package com.yourname.rbxleditor;

import android.content.Context;
import android.view.View;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputMethodManager;

/**
 * A tiny (1x1 px) focusable View whose only job is to own an
 * {@link InputConnection} so the soft keyboard / Gboard has somewhere to
 * send commitText (typing) and paste events. egui renders its own text
 * and NativeActivity provides no EditText, so without this the IME has
 * nowhere to deliver input.
 *
 * It is NOT focusable in touch mode so taps pass through to the Bevy
 * surface; focus is requested programmatically from Rust when an egui
 * text field gains focus.
 */
public class InputTargetView extends View {
    public InputTargetView(Context context) {
        super(context);
        setFocusable(true);
        // Do NOT setFocusableInTouchMode(true): that would let this 1px
        // view steal touches meant for the egui UI and cause the
        // keyboard to flicker open/closed on every tap.
        setFocusableInTouchMode(false);
        setClickable(false);
        setLongClickable(false);
    }

    @Override
    public boolean onCheckIsTextEditor() {
        // Only act as a text editor (and thus show a keyboard) when
        // we've been explicitly focused by Rust.
        return hasFocus();
    }

    @Override
    public InputConnection onCreateInputConnection(EditorInfo outAttrs) {
        outAttrs.inputType = EditorInfo.TYPE_CLASS_TEXT
                | EditorInfo.TYPE_TEXT_FLAG_MULTI_LINE
                | EditorInfo.TYPE_TEXT_FLAG_NO_SUGGESTIONS;
        outAttrs.imeOptions = EditorInfo.IME_ACTION_DONE;
        return new RbxInputConnection(this);
    }

    /** Called from Rust when an egui text field gains focus. */
    public void showIme() {
        post(() -> {
            requestFocus();
            InputMethodManager imm =
                    (InputMethodManager) getContext().getSystemService(Context.INPUT_METHOD_SERVICE);
            if (imm != null) imm.showSoftInput(this, InputMethodManager.SHOW_FORCED);
        });
    }

    /** Called from Rust when focus leaves all egui text fields. */
    public void hideIme() {
        post(() -> {
            InputMethodManager imm =
                    (InputMethodManager) getContext().getSystemService(Context.INPUT_METHOD_SERVICE);
            if (imm != null) imm.hideSoftInputFromWindow(getWindowToken(), 0);
            clearFocus();
        });
    }
}
