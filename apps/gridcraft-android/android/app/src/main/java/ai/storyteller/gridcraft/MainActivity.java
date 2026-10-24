package ai.storyteller.gridcraft;

import com.google.androidgamesdk.GameActivity;

import android.os.Bundle;

/**
 * GameActivity host for the Rust cdylib.
 *
 * GameActivity (not NativeActivity) is required: eframe's accesskit feature is a hard
 * `compile_error!` with android-native-activity, and NativeActivity has no soft keyboard / IME,
 * so a panel + text-entry app would be unusable.
 *
 * `android.app.lib_name` in AndroidManifest.xml is "main", which must match
 * `[lib] name = "main"` in the Rust crate.
 */
public class MainActivity extends GameActivity {

    static {
        System.loadLibrary("main");
    }

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
    }
}
