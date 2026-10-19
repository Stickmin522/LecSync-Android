package com.lecsync.desk

import android.content.Context
import android.content.Intent
import android.media.AudioManager
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.EventChannel
import io.flutter.plugin.common.MethodChannel
import org.json.JSONObject
import java.util.concurrent.Executors

class Bridge(private val context: Context, engine: FlutterEngine, private val activity: MainActivity? = null) {
    private var sink: EventChannel.EventSink? = null
    private val event = EventChannel(engine.dartExecutor.binaryMessenger, "desk/events")
    private val channel = MethodChannel(engine.dartExecutor.binaryMessenger, "desk/methods")
    private val overlayChannel = MethodChannel(engine.dartExecutor.binaryMessenger, "desk/overlay")
    init {
        instances.add(this)
        event.setStreamHandler(object : EventChannel.StreamHandler {
            override fun onListen(arguments: Any?, events: EventChannel.EventSink?) { sink = events; events?.success(ChineseDigest.decorate(context,JSONObject(NativeCore.command("{\"op\":\"snapshot\"}"))).toString()) }
            override fun onCancel(arguments: Any?) { sink = null }
        })
        channel.setMethodCallHandler { call, result ->
            try {
                val input = JSONObject(call.arguments as? String ?: "{}")
                when (call.method) {
                    "settings" -> result.success(SettingsStore.read(context).toString())
                    "saveSettings" -> { SettingsStore.save(context, input); broadcast(); result.success(SettingsStore.read(context).toString()) }
                    "start" -> if (activity != null) activity.requestStart(input.optString("title"), result) else result.error("foreground", "请返回主界面开始新录音", null)
                    "pause", "stop" -> if(activity!=null) { SessionService.instance?.control(call.method); result.success("{}") }else result.error("foreground","请在应用内控制录音",null)
                    "resume" -> if(activity!=null)activity.requestResume(result)else result.error("foreground","请回到应用继续录音",null)
                    "overlay" -> if (activity != null) activity.setOverlay(input.optBoolean("enabled"), result) else {
                        result.error("foreground","请在应用内设置悬浮字幕",null)
                    }
                    "openLecSyncConsole" -> if(activity != null) { activity.openLecSyncConsole(); result.success("{}") } else result.error("foreground","请在设置页面打开控制台",null)
                    "closeOverlay" -> { SessionService.instance?.dismissOverlay(); result.success("{}") }
                    "overlayControls" -> { SessionService.instance?.setOverlayControlsVisible(input.optBoolean("visible")); result.success("{}") }
                    "overlayTextInput" -> { SessionService.instance?.setOverlayTextInput(input.optBoolean("enabled")); result.success("{}") }
                    "digest", "digestRetry" -> {
                        input.put("op","load").put("directory",SettingsStore.directory(context))
                        executor.execute{val view=JSONObject(NativeCore.command(input.toString()));handler.post{result.success(ChineseDigest.decorate(context,view,call.method=="digestRetry").toString())}}
                    }
                    "open" -> { context.startActivity(Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_REORDER_TO_FRONT)); result.success("{}") }
                    "devices" -> { val am=context.getSystemService(AudioManager::class.java); val array=org.json.JSONArray(); for(d in am.getDevices(AudioManager.GET_DEVICES_INPUTS)) array.put(JSONObject().put("id",d.id.toString()).put("name",d.productName.toString())); result.success(array.toString()) }
                    "export" -> if(activity != null) activity.exportRecord(input,result) else result.error("foreground","请在主界面导出",null)
                    "play" -> if(activity != null) { activity.playRecord(input); result.success("{}") } else result.error("foreground","请在主界面回放",null)
                    "stopPlayback" -> { activity?.stopPlayback(); result.success("{}") }
                    "core" -> {
                        input.put("directory", SettingsStore.directory(context))
                        if(input.optString("op")=="test") input.put("config",SettingsStore.config(context))
                        if(input.optString("op")=="retranslateCaption") input.put("apiKey",runCatching { SettingsStore.readKey(context) }.getOrDefault(""))
                        executor.execute {
                            val output=runCatching { NativeCore.command(input.toString()) }
                            handler.post { output.fold({ result.success(it); if(input.optString("op") in setOf("editCaption", "retranslateCaption") && JSONObject(it).has("id")) broadcast() },{ result.error("core","底层操作失败",null) }) }
                        }
                    }
                    else -> result.notImplemented()
                }
            } catch(e: Exception) { result.error("operation",e.message ?: "操作失败",null) }
        }
    }
    fun overlayInteraction(active: Boolean) { overlayChannel.invokeMethod("interaction", active) }
    fun overlayShown() { overlayChannel.invokeMethod("shown", null) }
    fun close() { instances.remove(this); channel.setMethodCallHandler(null); event.setStreamHandler(null); sink=null }
    companion object {
        private val instances = java.util.concurrent.CopyOnWriteArrayList<Bridge>()
        private val executor = Executors.newCachedThreadPool()
        private val handler = Handler(Looper.getMainLooper())
        fun broadcast() { val raw=JSONObject(NativeCore.command("{\"op\":\"snapshot\"}")); for(b in instances) b.sink?.success(ChineseDigest.decorate(b.context,JSONObject(raw.toString())).toString()) }
    }
}
