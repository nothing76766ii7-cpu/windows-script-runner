package com.vanta.windowsscriptrunner

import android.content.Context
import android.os.Handler
import android.os.Looper
import android.webkit.JavascriptInterface
import android.webkit.WebView
import org.json.JSONArray
import org.json.JSONObject
import java.io.BufferedReader
import java.io.File
import java.io.InputStreamReader

class AndroidBridge(private val context: Context, private val webView: WebView) {

    private val mainHandler = Handler(Looper.getMainLooper())

    @JavascriptInterface
    fun runCommand(cmd: String, callbackId: String): String {
        Thread {
            val result = executeShell(cmd)
            mainHandler.post {
                val escapedOutput = JSONObject.quote(result.output)
                val js = "window.__onBridgeResponse && window.__onBridgeResponse('$callbackId', { output: $escapedOutput, exit: ${result.exitCode} });"
                webView.evaluateJavascript(js, null)
            }
        }.start()
        return "async"
    }

    @JavascriptInterface
    fun listDirectory(path: String): String {
        return try {
            val resolved = resolvePath(path)
            val dir = File(resolved)
            if (!dir.exists() || !dir.isDirectory) {
                return JSONObject().put("error", "Not a directory").toString()
            }
            val filesArray = JSONArray()
            dir.listFiles()?.forEach { file ->
                val obj = JSONObject()
                obj.put("name", file.name)
                obj.put("isDir", file.isDirectory)
                obj.put("size", file.length())
                obj.put("lastModified", file.lastModified())
                filesArray.put(obj)
            }
            JSONObject().put("files", filesArray).toString()
        } catch (e: Exception) {
            JSONObject().put("error", e.message ?: "Unknown error").toString()
        }
    }

    @JavascriptInterface
    fun readFile(path: String): String {
        return try {
            val file = File(resolvePath(path))
            if (!file.exists()) return JSONObject().put("error", "File not found").toString()
            JSONObject().put("content", file.readText()).toString()
        } catch (e: Exception) {
            JSONObject().put("error", e.message ?: "Unknown error").toString()
        }
    }

    private fun resolvePath(path: String): String {
        var p = path.trim()
        if (p == "~" || p.startsWith("~/")) {
            p = "/sdcard" + p.substring(1)
        }
        return p
    }

    private data class ShellResult(val output: String, val exitCode: Int)

    private fun executeShell(command: String): ShellResult {
        return try {
            val process = Runtime.getRuntime().exec(arrayOf("sh", "-c", command))
            val reader = BufferedReader(InputStreamReader(process.inputStream))
            val errReader = BufferedReader(InputStreamReader(process.errorStream))
            val sb = StringBuilder()
            var line: String?

            while (reader.readLine().also { line = it } != null) {
                sb.append(line).append("\n")
            }
            val errSb = StringBuilder()
            while (errReader.readLine().also { line = it } != null) {
                errSb.append(line).append("\n")
            }

            val exit = process.waitFor()
            if (errSb.isNotEmpty()) {
                if (sb.isNotEmpty()) sb.append("\n")
                sb.append("[ERR] ").append(errSb)
            }
            ShellResult(sb.toString().trimEnd(), exit)
        } catch (e: Exception) {
            ShellResult("[ERROR] " + (e.message ?: "Execution failed"), 1)
        }
    }
}
