package com.lecsync.desk

import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import org.junit.Assert.*
import org.junit.Test

class OverlayPlatformTest {
    private class Clipboard : OverlayClipboard {
        var text: String? = null
        var reads = 0
        var writes = 0
        var fail = false
        override fun write(text: String) {
            if (fail) throw SecurityException("sensitive details")
            this.text = text
            writes++
        }
        override fun read(): String? { reads++; return text }
        override fun hasText() = text != null
    }

    private class Reply : MethodChannel.Result {
        var completions = 0
        var value: Any? = null
        var error: String? = null
        var message: String? = null
        var unsupported = false
        override fun success(result: Any?) { completions++; value = result }
        override fun error(code: String, message: String?, details: Any?) {
            completions++; error = code; this.message = message
        }
        override fun notImplemented() { completions++; unsupported = true }
    }

    @Test fun copyAndPasteCompleteWithOnlyTheSelectedText() {
        val clipboard = Clipboard()
        val handler = OverlayPlatformHandler(clipboard)
        val copy = Reply()
        handler.onMethodCall(MethodCall("Clipboard.setData", mapOf("text" to "selected paragraph")), copy)
        assertEquals(1, copy.completions); assertNull(copy.error)
        assertEquals("selected paragraph", clipboard.text); assertEquals(1, clipboard.writes)
        val paste = Reply()
        handler.onMethodCall(MethodCall("Clipboard.getData", "text/plain"), paste)
        assertEquals(1, paste.completions)
        assertEquals(mapOf("text" to "selected paragraph"), paste.value)
    }

    @Test fun hasStringsChecksAvailabilityWithoutReadingClipboardContent() {
        val clipboard = Clipboard().apply { text = "private paragraph" }
        val reply = Reply()
        OverlayPlatformHandler(clipboard).onMethodCall(MethodCall("Clipboard.hasStrings", null), reply)
        assertEquals(1, reply.completions)
        assertEquals(mapOf("value" to true), reply.value); assertEquals(0, clipboard.reads)
    }

    @Test fun emptyClipboardReturnsNullAndUnsupportedFormatCompletesWithError() {
        val clipboard = Clipboard()
        val handler = OverlayPlatformHandler(clipboard)
        val empty = Reply()
        handler.onMethodCall(MethodCall("Clipboard.getData", "text/plain"), empty)
        assertEquals(1, empty.completions); assertNull(empty.value); assertNull(empty.error)
        val unsupported = Reply()
        handler.onMethodCall(MethodCall("Clipboard.getData", "image/png"), unsupported)
        assertEquals(1, unsupported.completions); assertEquals("clipboard", unsupported.error)
        assertEquals(1, clipboard.reads)
    }

    @Test fun deniedCopyCompletesWithErrorWithoutLeakingContent() {
        val clipboard = Clipboard().apply { fail = true }
        val reply = Reply()
        OverlayPlatformHandler(clipboard).onMethodCall(
            MethodCall("Clipboard.setData", mapOf("text" to "private paragraph")), reply)
        assertEquals(1, reply.completions); assertEquals("platform", reply.error)
        assertFalse(reply.message.orEmpty().contains("sensitive"))
        assertFalse(reply.message.orEmpty().contains("private paragraph"))
    }

    @Test fun textEditingFeedbackAndActivityOnlyCallsAlwaysComplete() {
        val handler = OverlayPlatformHandler(Clipboard())
        for (method in listOf("SystemSound.play", "HapticFeedback.vibrate",
            "SystemChrome.setPreferredOrientations", "SystemChrome.setApplicationSwitcherDescription",
            "SystemChrome.setEnabledSystemUIOverlays", "SystemChrome.setEnabledSystemUIMode",
            "SystemChrome.setSystemUIChangeListener", "SystemChrome.restoreSystemUIOverlays",
            "SystemChrome.setSystemUIOverlayStyle", "SystemNavigator.setFrameworkHandlesBack",
            "SystemNavigator.pop")) {
            val reply = Reply()
            handler.onMethodCall(MethodCall(method, null), reply)
            assertEquals(method, 1, reply.completions); assertNull(method, reply.error)
        }
    }

    @Test fun unknownPlatformCallsCompleteWithoutHanging() {
        val reply = Reply()
        OverlayPlatformHandler(Clipboard()).onMethodCall(MethodCall("unknown", null), reply)
        assertEquals(1, reply.completions); assertTrue(reply.unsupported)
    }
}
