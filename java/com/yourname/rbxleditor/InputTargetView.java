package com.yourname.rbxleditor;

import android.content.Context;
import android.view.MotionEvent;
import android.view.View;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputMethodManager;

/**
 * Hosts the {@link InputConnection} that lets the soft keyboard /
 * Gboard deliver typing and paste to egui's self-rendered text fields.
 *
 * The view fills the window but is transparent and does NOT consume
 * touches: pointer events still reach the Bevy/egui surface underneath.
 * It MUST be focusable in touch mode so Android will create an
 * InputConnection for it when we request focus + showSoftInput.
 */
public class InputTargetView extends View {
    public InputTargetView(Context context) {
        super(context);
        setFocusable(true);
        setFocusableInTouchMode(true);
        // Critical: without these, the 1px/transparent view is treated
        // as not eligible for IME focus on many devices.
        setVisibility(View.VISIBLE);
        setEnabled(true);
        // Don't intercept touches — let them reach Bevy/egui.
        setClickable(false);
        setLongClickable(false);
    }

    @Override
    public boolean onCheckIsTextEditor() {
        // We are always a text editor while we have focus. Returning
        // true unconditionally makes the IME bind an InputConnection.
        return true;
    }

    @Override
    public InputConnection onCreateInputConnection(EditorInfo outAttrs) {
        outAttrs.inputType = EditorInfo.TYPE_CLASS_TEXT
                | EditorInfo.TYPE_TEXT_FLAG_MULTI_LINE
                | EditorInfo.TYPE_TEXT_FLAG_NO_SUGGESTIONS;
        outAttrs.imeOptions = EditorInfo.IME_ACTION_DONE;
        // Some IMEs refuse to commit text unless the selection is set.
        outAttrs.initialSelStart = 0;
        outAttrs.initialSelEnd = 0;
        return new RbxInputConnection(this);
    }

    @Override
    public boolean onTouchEvent(MotionEvent event) {
        // Never consume touches; pass them through to the Bevy surface.
        return false;
    }

    /** Rust calls this when an egui text field gains focus. */
    public void showIme() {
        post(() -> {
            requestFocus();
            InputMethodManager imm =
                    (InputMethodManager) getContext().getSystemService(Context.INPUT_METHOD_SERVICE);
            if (imm == null) return;
            // Force the IME to (re)create the InputConnection, then
            // show. restartInput is important after focus changes.
            imm.restartInput(this);
            imm.showSoftInput(this, InputMethodManager.SHOW_FORCED);
        });
    }

    /** Rust calls this when focus leaves all egui text fields. */
    public void hideIme() {
        post(() -> {
            InputMethodManager imm =
                    (InputMethodManager) getContext().getSystemService(Context.INPUT_METHOD_SERVICE);
            if (imm != null) {
                imm.hideSoftInputFromWindow(getWindowToken(), 0);
            }
            clearFocus();
        });
    }

}
