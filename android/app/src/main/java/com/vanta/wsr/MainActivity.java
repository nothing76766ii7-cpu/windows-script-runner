package com.vanta.wsr;

import android.app.Activity;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.os.Bundle;
import android.provider.OpenableColumns;
import android.system.Os;
import android.webkit.JavascriptInterface;
import android.webkit.ValueCallback;
import android.webkit.WebChromeClient;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;

public class MainActivity extends Activity {
    private static final int REQ_SAF_PICK_FILE = 1001;
    private WebView webView;
    private File vfsRoot;
    private String pendingSafTargetWinDir = "C:\\Users\\Admin\\Downloads";

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        try {
            vfsRoot = new File(getFilesDir(), ".wsr_vfs");
            if (!vfsRoot.exists()) {
                vfsRoot.mkdirs();
            }
            Os.setenv("HOME", getFilesDir().getAbsolutePath(), true);
            Os.setenv("WSR_VFS_ROOT", vfsRoot.getAbsolutePath(), true);
        } catch (Exception ignored) {
        }

        try {
            RustBridge.initVfs();
            RustBridge.startServer(18765);
        } catch (Throwable ignored) {
        }

        webView = new WebView(this);
        WebSettings settings = webView.getSettings();
        settings.setJavaScriptEnabled(true);
        settings.setDomStorageEnabled(true);
        settings.setAllowFileAccess(true);
        settings.setAllowContentAccess(true);
        settings.setMixedContentMode(WebSettings.MIXED_CONTENT_ALWAYS_ALLOW);

        webView.addJavascriptInterface(new SafBridge(), "AndroidSAF");
        webView.setWebViewClient(new WebViewClient());
        webView.setWebChromeClient(new WebChromeClient());
        setContentView(webView);

        webView.loadUrl("file:///android_asset/index.html");
    }

    public final class SafBridge {
        @JavascriptInterface
        public void pickFile(String currentWinDir) {
            if (currentWinDir != null && !currentWinDir.trim().isEmpty()) {
                pendingSafTargetWinDir = currentWinDir.trim();
            }
            Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
            intent.addCategory(Intent.CATEGORY_OPENABLE);
            intent.setType("*/*");
            startActivityForResult(intent, REQ_SAF_PICK_FILE);
        }
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode == REQ_SAF_PICK_FILE && resultCode == Activity.RESULT_OK && data != null) {
            Uri uri = data.getData();
            if (uri != null) {
                importSafUriToVfs(uri);
            }
        }
    }

    private void importSafUriToVfs(Uri uri) {
        try {
            String fileName = queryDisplayName(uri);
            if (fileName == null || fileName.trim().isEmpty()) {
                fileName = "imported_script.bat";
            }
            File driveC = new File(vfsRoot, "drive_c");
            File destDir = new File(driveC, "Users/Admin/Downloads");
            if (pendingSafTargetWinDir.toUpperCase().startsWith("C:\\")) {
                String rel = pendingSafTargetWinDir.substring(3).replace('\\', '/');
                if (!rel.isEmpty()) {
                    destDir = new File(driveC, rel);
                }
            }
            if (!destDir.exists()) {
                destDir.mkdirs();
            }
            File destFile = new File(destDir, fileName);
            try (InputStream in = getContentResolver().openInputStream(uri);
                 FileOutputStream out = new FileOutputStream(destFile)) {
                if (in != null) {
                    byte[] buf = new byte[8192];
                    int n;
                    while ((n = in.read(buf)) > 0) {
                        out.write(buf, 0, n);
                    }
                }
            }
            final String winPath = pendingSafTargetWinDir.replaceAll("[\\\\/]+$", "") + "\\" + fileName;
            runOnUiThread(() -> {
                if (webView != null) {
                    String escaped = winPath.replace("\\", "\\\\").replace("'", "\\'");
                    webView.evaluateJavascript("window.onSafFileImported && window.onSafFileImported('" + escaped + "')", null);
                }
            });
        } catch (Exception ignored) {
        }
    }

    private String queryDisplayName(Uri uri) {
        try (Cursor cursor = getContentResolver().query(uri, null, null, null, null)) {
            if (cursor != null && cursor.moveToFirst()) {
                int idx = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME);
                if (idx >= 0) {
                    return cursor.getString(idx);
                }
            }
        } catch (Exception ignored) {
        }
        return uri.getLastPathSegment();
    }

    @Override
    public void onBackPressed() {
        if (webView != null) {
            webView.evaluateJavascript(
                "(function(){ return !!(window.handleAndroidBack && window.handleAndroidBack()); })()",
                new ValueCallback<String>() {
                    @Override
                    public void onReceiveValue(String value) {
                        if (!"true".equals(value)) {
                            MainActivity.super.onBackPressed();
                        }
                    }
                }
            );
        } else {
            super.onBackPressed();
        }
    }
}
