package com.vanta.wsr;

import android.app.Activity;
import android.content.ContentResolver;
import android.content.Context;
import android.content.Intent;
import android.content.SharedPreferences;
import android.database.Cursor;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.provider.DocumentsContract;
import android.provider.MediaStore;
import android.provider.OpenableColumns;
import android.system.Os;
import android.webkit.JavascriptInterface;
import android.webkit.ValueCallback;
import android.webkit.WebChromeClient;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;

public class MainActivity extends Activity {
    private static final int REQ_SAF_PICK_FILE = 1001;
    private static final int REQ_SAF_PICK_TREE = 1002;
    private static final int REQ_SAF_EXPORT_FILE = 1003;
    private static final String PREFS_NAME = "wsr_saf_mounts";

    private WebView webView;
    private File vfsRoot;
    private String pendingSafSlot = "downloads";
    private String pendingSafTargetWinDir = "C:\\Users\\Admin\\Downloads";
    private String pendingExportSourceWinPath = "";

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

    private SharedPreferences getSafPrefs() {
        return getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE);
    }

    public final class SafBridge {
        @JavascriptInterface
        public void pickFolderTree(String slot) {
            pendingSafSlot = (slot == null || slot.trim().isEmpty()) ? "downloads" : slot.trim().toLowerCase();
            Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT_TREE);
            intent.addFlags(
                Intent.FLAG_GRANT_READ_URI_PERMISSION |
                Intent.FLAG_GRANT_WRITE_URI_PERMISSION |
                Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION |
                Intent.FLAG_GRANT_PREFIX_URI_PERMISSION
            );
            startActivityForResult(intent, REQ_SAF_PICK_TREE);
        }

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

        @JavascriptInterface
        public void exportFileToAndroid(String sourceWinPath, String suggestedName) {
            pendingExportSourceWinPath = sourceWinPath != null ? sourceWinPath : "";
            Intent intent = new Intent(Intent.ACTION_CREATE_DOCUMENT);
            intent.addCategory(Intent.CATEGORY_OPENABLE);
            intent.setType("*/*");
            intent.putExtra(Intent.EXTRA_TITLE, (suggestedName != null && !suggestedName.isEmpty()) ? suggestedName : "exported_file.txt");
            startActivityForResult(intent, REQ_SAF_EXPORT_FILE);
        }

        @JavascriptInterface
        public String getPersistedTreeUri(String slot) {
            if (slot == null) return "";
            String s = slot.trim().toLowerCase();
            String uriStr = getSafPrefs().getString("tree_" + s, "");
            if (uriStr == null || uriStr.isEmpty()) return "";
            return uriStr;
        }

        @JavascriptInterface
        public String listSafFolderJson(String slot) {
            String s = (slot == null) ? "downloads" : slot.trim().toLowerCase();
            String treeUriStr = getSafPrefs().getString("tree_" + s, "");
            if (treeUriStr == null || treeUriStr.isEmpty()) {
                return "{\"status\":\"unconnected\",\"slot\":\"" + jsEsc(s) + "\"}";
            }
            try {
                Uri treeUri = Uri.parse(treeUriStr);
                String docId = DocumentsContract.getTreeDocumentId(treeUri);
                Uri childrenUri = DocumentsContract.buildChildDocumentsUriUsingTree(treeUri, docId);
                String[] proj = new String[] {
                    DocumentsContract.Document.COLUMN_DOCUMENT_ID,
                    DocumentsContract.Document.COLUMN_DISPLAY_NAME,
                    DocumentsContract.Document.COLUMN_MIME_TYPE,
                    DocumentsContract.Document.COLUMN_SIZE,
                    DocumentsContract.Document.COLUMN_LAST_MODIFIED
                };
                StringBuilder items = new StringBuilder();
                int count = 0;
                try (Cursor c = getContentResolver().query(childrenUri, proj, null, null, null)) {
                    if (c != null) {
                        while (c.moveToNext()) {
                            String childId = c.getString(0);
                            String name = c.getString(1);
                            String mime = c.getString(2);
                            long size = c.isNull(3) ? 0L : c.getLong(3);
                            long mod = c.isNull(4) ? 0L : c.getLong(4);
                            boolean isDir = DocumentsContract.Document.MIME_TYPE_DIR.equals(mime);
                            Uri docUri = DocumentsContract.buildDocumentUriUsingTree(treeUri, childId);
                            if (count > 0) items.append(",");
                            items.append("{")
                                 .append("\"name\":\"").append(jsEsc(name)).append("\",")
                                 .append("\"uri\":\"").append(jsEsc(docUri.toString())).append("\",")
                                 .append("\"is_dir\":").append(isDir).append(",")
                                 .append("\"size\":").append(size).append(",")
                                 .append("\"modified\":").append(mod).append(",")
                                 .append("\"is_real_android\":true")
                                 .append("}");
                            count++;
                        }
                    }
                }
                return "{\"status\":\"ok\",\"slot\":\"" + jsEsc(s) + "\",\"tree_uri\":\"" + jsEsc(treeUriStr) + "\",\"entries\":[" + items.toString() + "]}";
            } catch (SecurityException se) {
                return "{\"status\":\"permission_revoked\",\"slot\":\"" + jsEsc(s) + "\",\"error\":\"" + jsEsc(se.getMessage()) + "\"}";
            } catch (Exception e) {
                return "{\"status\":\"error\",\"slot\":\"" + jsEsc(s) + "\",\"error\":\"" + jsEsc(e.getMessage()) + "\"}";
            }
        }

        @JavascriptInterface
        public String listMediaStoreJson(String collection) {
            String col = (collection == null) ? "downloads" : collection.trim().toLowerCase();
            Uri contentUri;
            if ("pictures".equals(col)) {
                contentUri = MediaStore.Images.Media.EXTERNAL_CONTENT_URI;
            } else if ("music".equals(col)) {
                contentUri = MediaStore.Audio.Media.EXTERNAL_CONTENT_URI;
            } else if ("movies".equals(col)) {
                contentUri = MediaStore.Video.Media.EXTERNAL_CONTENT_URI;
            } else if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                contentUri = MediaStore.Downloads.EXTERNAL_CONTENT_URI;
            } else {
                contentUri = MediaStore.Files.getContentUri("external");
            }
            String[] proj = new String[] {
                MediaStore.MediaColumns._ID,
                MediaStore.MediaColumns.DISPLAY_NAME,
                MediaStore.MediaColumns.SIZE,
                MediaStore.MediaColumns.DATE_MODIFIED
            };
            StringBuilder items = new StringBuilder();
            int count = 0;
            try (Cursor c = getContentResolver().query(contentUri, proj, null, null, MediaStore.MediaColumns.DATE_MODIFIED + " DESC")) {
                if (c != null) {
                    while (c.moveToNext() && count < 200) {
                        long id = c.getLong(0);
                        String name = c.getString(1);
                        long size = c.isNull(2) ? 0L : c.getLong(2);
                        long mod = c.isNull(3) ? 0L : c.getLong(3) * 1000L;
                        Uri itemUri = Uri.withAppendedPath(contentUri, String.valueOf(id));
                        if (count > 0) items.append(",");
                        items.append("{")
                             .append("\"name\":\"").append(jsEsc(name)).append("\",")
                             .append("\"uri\":\"").append(jsEsc(itemUri.toString())).append("\",")
                             .append("\"is_dir\":false,")
                             .append("\"size\":").append(size).append(",")
                             .append("\"modified\":").append(mod).append(",")
                             .append("\"is_real_android\":true")
                             .append("}");
                        count++;
                    }
                }
                return "{\"status\":\"ok\",\"collection\":\"" + jsEsc(col) + "\",\"entries\":[" + items.toString() + "]}";
            } catch (Exception e) {
                return "{\"status\":\"error\",\"error\":\"" + jsEsc(e.getMessage()) + "\"}";
            }
        }

        @JavascriptInterface
        public String stageContentUriToVfs(String uriStr, String fileName) {
            try {
                Uri uri = Uri.parse(uriStr);
                String safeName = (fileName == null || fileName.isEmpty()) ? queryDisplayName(uri) : fileName;
                if (safeName == null || safeName.isEmpty()) safeName = "staged_file.bat";
                safeName = safeName.replaceAll("[^a-zA-Z0-9._-]", "_");

                File stageDir = new File(vfsRoot, "drive_c/Temp/android_stage");
                if (!stageDir.exists()) stageDir.mkdirs();
                File dst = new File(stageDir, safeName);

                try (InputStream in = getContentResolver().openInputStream(uri);
                     FileOutputStream out = new FileOutputStream(dst)) {
                    if (in == null) {
                        return "{\"status\":\"error\",\"error\":\"ContentResolver returned null stream\"}";
                    }
                    byte[] buf = new byte[16384];
                    int n;
                    while ((n = in.read(buf)) > 0) {
                        out.write(buf, 0, n);
                    }
                }
                String winPath = "C:\\Temp\\android_stage\\" + safeName;
                return "{\"status\":\"ok\",\"staged_win_path\":\"" + jsEsc(winPath) + "\",\"android_path\":\"" + jsEsc(dst.getAbsolutePath()) + "\"}";
            } catch (Exception e) {
                return "{\"status\":\"error\",\"error\":\"" + jsEsc(e.getMessage()) + "\"}";
            }
        }

        @JavascriptInterface
        public String createSafSubfolder(String slot, String folderName) {
            try {
                String treeUriStr = getSafPrefs().getString("tree_" + slot.toLowerCase(), "");
                if (treeUriStr.isEmpty()) return "{\"status\":\"error\",\"error\":\"Folder not connected via SAF\"}";
                Uri treeUri = Uri.parse(treeUriStr);
                String docId = DocumentsContract.getTreeDocumentId(treeUri);
                Uri parentDocUri = DocumentsContract.buildDocumentUriUsingTree(treeUri, docId);
                Uri created = DocumentsContract.createDocument(
                    getContentResolver(),
                    parentDocUri,
                    DocumentsContract.Document.MIME_TYPE_DIR,
                    folderName
                );
                if (created != null) {
                    return "{\"status\":\"ok\",\"uri\":\"" + jsEsc(created.toString()) + "\"}";
                }
                return "{\"status\":\"error\",\"error\":\"Provider refused folder creation\"}";
            } catch (Exception e) {
                return "{\"status\":\"error\",\"error\":\"" + jsEsc(e.getMessage()) + "\"}";
            }
        }

        @JavascriptInterface
        public String deleteSafDocument(String docUriStr) {
            try {
                Uri docUri = Uri.parse(docUriStr);
                boolean ok = DocumentsContract.deleteDocument(getContentResolver(), docUri);
                return ok ? "{\"status\":\"ok\"}" : "{\"status\":\"error\",\"error\":\"Provider refused delete\"}";
            } catch (Exception e) {
                return "{\"status\":\"error\",\"error\":\"" + jsEsc(e.getMessage()) + "\"}";
            }
        }

        @JavascriptInterface
        public String renameSafDocument(String docUriStr, String newName) {
            try {
                Uri docUri = Uri.parse(docUriStr);
                Uri renamed = DocumentsContract.renameDocument(getContentResolver(), docUri, newName);
                return renamed != null
                    ? "{\"status\":\"ok\",\"uri\":\"" + jsEsc(renamed.toString()) + "\"}"
                    : "{\"status\":\"error\",\"error\":\"Provider refused rename\"}";
            } catch (Exception e) {
                return "{\"status\":\"error\",\"error\":\"" + jsEsc(e.getMessage()) + "\"}";
            }
        }
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (resultCode != Activity.RESULT_OK || data == null) return;
        Uri uri = data.getData();
        if (uri == null) return;

        if (requestCode == REQ_SAF_PICK_TREE) {
            try {
                final int takeFlags = data.getFlags() & (Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION);
                getContentResolver().takePersistableUriPermission(uri, takeFlags);
                getSafPrefs().edit().putString("tree_" + pendingSafSlot, uri.toString()).apply();
                final String slot = pendingSafSlot;
                final String uriStr = uri.toString();
                runOnUiThread(() -> {
                    if (webView != null) {
                        webView.evaluateJavascript(
                            "window.onSafTreeConnected && window.onSafTreeConnected('" + jsEsc(slot) + "', '" + jsEsc(uriStr) + "')",
                            null
                        );
                    }
                });
            } catch (Exception ignored) {
            }
        } else if (requestCode == REQ_SAF_PICK_FILE) {
            importSafUriToVfs(uri);
        } else if (requestCode == REQ_SAF_EXPORT_FILE) {
            exportVfsFileToSafUri(pendingExportSourceWinPath, uri);
        }
    }

    private void importSafUriToVfs(Uri uri) {
        try {
            String fileName = queryDisplayName(uri);
            if (fileName == null || fileName.trim().isEmpty()) {
                fileName = "imported_script.bat";
            }
            File driveC = new File(vfsRoot, "drive_c");
            File destDir = new File(driveC, "Users/Admin/Desktop");
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
                    byte[] buf = new byte[16384];
                    int n;
                    while ((n = in.read(buf)) > 0) {
                        out.write(buf, 0, n);
                    }
                }
            }
            final String winPath = pendingSafTargetWinDir.replaceAll("[\\\\/]+$", "") + "\\" + fileName;
            runOnUiThread(() -> {
                if (webView != null) {
                    webView.evaluateJavascript("window.onSafFileImported && window.onSafFileImported('" + jsEsc(winPath) + "')", null);
                }
            });
        } catch (Exception ignored) {
        }
    }

    private void exportVfsFileToSafUri(String srcWinPath, Uri destUri) {
        try {
            File driveC = new File(vfsRoot, "drive_c");
            String rel = srcWinPath.replaceFirst("^[Cc]:[\\\\/]*", "").replace('\\', '/');
            File srcFile = new File(driveC, rel);
            if (!srcFile.exists()) return;
            try (InputStream in = new FileInputStream(srcFile);
                 OutputStream out = getContentResolver().openOutputStream(destUri)) {
                if (out != null) {
                    byte[] buf = new byte[16384];
                    int n;
                    while ((n = in.read(buf)) > 0) {
                        out.write(buf, 0, n);
                    }
                }
            }
            runOnUiThread(() -> {
                if (webView != null) {
                    webView.evaluateJavascript("window.onSafFileExported && window.onSafFileExported('" + jsEsc(srcWinPath) + "')", null);
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

    private static String jsEsc(String s) {
        if (s == null) return "";
        return s.replace("\\", "\\\\")
                .replace("\"", "\\\"")
                .replace("'", "\\'")
                .replace("\n", "\\n")
                .replace("\r", "\\r");
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
