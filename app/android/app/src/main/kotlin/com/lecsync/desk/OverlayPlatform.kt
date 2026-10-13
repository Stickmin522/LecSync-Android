package com.lecsync.desk

import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.Context
import android.os.PersistableBundle
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.JSONMethodCodec
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel

/** PlatformPlugin belongs to an Activity; the service's engine needs its own platform replies. */
internal class OverlayPlatform(context: Context, engine: FlutterEngine) {
    private val channel = MethodChannel(engine.dartExecutor.binaryMessenger,
        "flutter/platform", JSONMethodCodec.INSTANCE)

    init {
        channel.setMethodCallHandler(OverlayPlatformHandler(AndroidOverlayClipboard(context)))
    }

    fun close() { channel.setMethodCallHandler(null) }
}

internal interface OverlayClipboard {
    fun write(text: String)
    fun read(): String?
    fun hasText(): Boolean
}

private class AndroidOverlayClipboard(context: Context) : OverlayClipboard {
    private val clipboard = context.getSystemService(ClipboardManager::class.java)

    override fun write(text: String) {
        // Keep transcript text out of the label and Android's clipboard preview.
        val clip = ClipData.newPlainText("LingoDesk", text)
        clip.description.extras = PersistableBundle().apply {
            putBoolean(ClipDescription.EXTRA_IS_SENSITIVE, true)
        }
        clipboard.setPrimaryClip(clip)
    }

    override fun read(): String? {
        // Paste is requested explicitly while the overlay has input focus. Do not
        // dereference clipboard URIs or request permissions for unrelated content.
        val clip = clipboard.primaryClip ?: return null
        return if (clip.itemCount > 0) clip.getItemAt(0).text?.toString() else null
    }

    override fun hasText(): Boolean =
        clipboard.hasPrimaryClip() && clipboard.primaryClipDescription?.hasMimeType("text/*") == true
}

/** Every request must complete, including unsupported Activity-only platform operations. */
internal class OverlayPlatformHandler(private val clipboard: OverlayClipboard) : MethodChannel.MethodCallHandler {
    override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
        try {
            when (call.method) {
                "Clipboard.setData" -> {
                    val text = call.argument<String>("text")
                    if (text == null) result.error("clipboard", "缺少复制内容", null)
                    else { clipboard.write(text); result.success(null) }
                }
                "Clipboard.getData" -> {
                    val format = call.arguments
                    if (format != null && format != "text/plain") {
                        result.error("clipboard", "不支持此剪贴板格式", null)
                    } else {
                        val text = clipboard.read()
                        result.success(text?.let { mapOf("text" to it) })
                    }
                }
                "Clipboard.hasStrings" -> result.success(mapOf("value" to clipboard.hasText()))
                // An overlay does not own the foreground app's Activity, system bars,
                // orientation or back stack. Acknowledge these calls without changing it.
                "SystemSound.play", "HapticFeedback.vibrate",
                "SystemChrome.setPreferredOrientations",
                "SystemChrome.setApplicationSwitcherDescription",
                "SystemChrome.setEnabledSystemUIOverlays",
                "SystemChrome.setEnabledSystemUIMode",
                "SystemChrome.setSystemUIChangeListener",
                "SystemChrome.restoreSystemUIOverlays",
                "SystemChrome.setSystemUIOverlayStyle",
                "SystemNavigator.setFrameworkHandlesBack",
                "SystemNavigator.pop" -> result.success(null)
                else -> result.notImplemented()
            }
        } catch (_: Exception) {
            // Never put copied text, clipboard content or exception details in logs/errors.
            result.error("platform", "悬浮窗操作失败，请重试", null)
        }
    }
}
