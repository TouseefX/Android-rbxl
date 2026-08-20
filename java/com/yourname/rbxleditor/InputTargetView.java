package com.yourname.rbxleditor;

import android.content.Context;
import android.view.View;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputMethodManager;

/**
 * Hosts the {@link InputConnection} for the soft keyboard.
 *
 * It fills the window so Android will bind an IME to it, but:
 *  - it returns false from onTouchEvent so touches reach Bevy,
 *  - it only reports itself as a text editor while it actually has
 *    focus (prevents the IME from re-opening in a focus loop),
 *  - show/hide are driven explicitly from Rust.
 */
public class InputTargetView extends View {
    public InputTargetView(Context context) {
        super(context);
        setFocusable(true);
        setFocusableInTouchMode(true);
        setVisibility(View.VISIBLE);
        setEnabled(true);
        setClickable(false);
        setLongClickable(false);
    }

    @Override
    public boolean onCheckIsTextEditor() {
        // Only act as an editor when Rust has focused us for text
        // input. Returning true unconditionally makes the IME fight
        // over focus and causes the keyboard to flicker.
        return hasFocus();
    }

    @Override
    public InputConnection onCreateInputConnection(EditorInfo outAttrs) {
        outAttrs.inputType = EditorInfo.TYPE_CLASS_TEXT
                | EditorInfo.TYPE_TEXT_FLAG_MULTI_LINE
                | EditorInfo.TYPE_TEXT_FLAG_NO_SUGGESTIONS;
        outAttrs.imeOptions = EditorInfo.IME_ACTION_DONE;
        outAttrs.initialSelStart = 0;
        outAttrs.initialSelEnd = 0;
        return new RbxInputConnection(this);
    }

    @Override
    public boolean onTouchEvent(MotionEvent event) {
        return false; // let Bevy/egui handle touches
    }

    public void showIme() {
        post(() -> {
            if (!requestFocus()) {
                // If we can't take focus now, the IME has nothing
                // to bind to; don't force-show (that causes loops).
                return;
            }
            InputMethodManager imm = (InputMethodManager)
                    getContext().getSystemService(Context.INPUT_METHOD_SERVICE);
            if (imm != null) {
                imm.showSoftInput(this, InputMethodManager.SHOW_IMPLICIT);
            }
        });
    }

    public void hideIme() {
        post(() -> {
            InputMethodManager imm = (InputMethodManager)
                    getContext().getSystemService(Context.INPUT_METHOD_SERVICE);
            if (imm != null && getWindowToken() != null) {
                imm.hideSoftInputFromWindow(getWindowToken(), 0);
            }
            clearFocus();
        });
    }
}
