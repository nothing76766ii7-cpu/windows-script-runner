package com.vanta.wsr;

import android.app.Activity;
import android.os.Bundle;
import android.system.Os;
import android.webkit.WebChromeClient;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import java.io.File;

public class MainActivity extends Activity {
    private WebView webView;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        try {
            File vfsRoot = new File(getFilesDir(), ".wsr_vfs");
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

        webView.setWebViewClient(new WebViewClient());
        webView.setWebChromeClient(new WebChromeClient());
        setContentView(webView);

        webView.loadUrl("file:///android_asset/index.html");
    }

    @Override
    public void onBackPressed() {
        if (webView != null && webView.canGoBack()) {
            webView.goBack();
        } else {
            super.onBackPressed();
        }
    }
}
