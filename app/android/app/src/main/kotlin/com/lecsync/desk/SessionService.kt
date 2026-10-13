package com.lecsync.desk

import android.app.*
import android.content.Intent
import android.content.pm.ServiceInfo
import android.content.res.Configuration
import android.graphics.PixelFormat
import android.media.*
import android.media.projection.MediaProjection
import android.media.projection.MediaProjectionManager
import android.os.*
import android.provider.Settings
import android.view.WindowManager
import io.flutter.FlutterInjector
import io.flutter.embedding.android.FlutterTextureView
import io.flutter.embedding.android.FlutterView
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.embedding.engine.dart.DartExecutor
import org.json.JSONObject
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicInteger
import kotlin.math.min

class SessionService : Service() {
    private val handler=Handler(Looper.getMainLooper())
    private val audioWorker=Executors.newSingleThreadExecutor()
    private val generation=AtomicInteger()
    @Volatile private var mic:AudioRecord?=null
    @Volatile private var system:AudioRecord?=null
    private var projection:MediaProjection?=null
    private var source="mic"
    private var device="default"
    private var previousPhase=""
    private var previousLanguage=""
    private var capturing=false
    private var overlayEngine:FlutterEngine?=null
    private var overlayBridge:Bridge?=null
    private var overlayPlatform:OverlayPlatform?=null
    private var overlayView:FlutterView?=null
    private var params:WindowManager.LayoutParams?=null
    private var overlayOriginalSoftInputMode:Int?=null
    private var overlayRoot:OverlayFrame?=null
    // addView registers the window before the first traversal attaches the view.
    private var overlayAdded=false
    private var overlayDismissed=false
    private var dockSide:OverlayDockSide?=null
    private var expandedGeometry:IntArray?=null
    private var recordingTypes=0
    private var sessionActive=false
    private var stoppingProjection=false
    private var wake:PowerManager.WakeLock?=null
    override fun onBind(intent:Intent?):IBinder?=null
    override fun onCreate(){super.onCreate();instance=this;wake=getSystemService(PowerManager::class.java).newWakeLock(PowerManager.PARTIAL_WAKE_LOCK,"$packageName:recording").apply{setReferenceCounted(false)};getSystemService(NotificationManager::class.java).createNotificationChannel(NotificationChannel("recording",AppLanguages.text(this,"录音与悬浮字幕"),NotificationManager.IMPORTANCE_LOW));handler.post(poll)}
    override fun onStartCommand(intent:Intent?,flags:Int,startId:Int):Int {
        try {
            when(intent?.action){
                "start"->{
                    val state=JSONObject(NativeCore.command("{\"op\":\"snapshot\"}"))
                    if(state.optString("phase") !in listOf("idle","ended"))return START_NOT_STICKY
                    val config=JSONObject(intent.getStringExtra("config")?:error("缺少配置"));source=config.optString("source","mic");device=config.optString("device","default")
                    sessionActive=true;overlayDismissed=false
                    recordingTypes=(if(source!="system"&&Build.VERSION.SDK_INT>=30)ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE else 0) or (if(source!="mic")ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PROJECTION else 0)
                    foreground("正在连接",recordingTypes)
                    if(source!="mic")grantProjection(intent)
                    previousPhase="";val response=JSONObject(NativeCore.command(JSONObject().put("op","start").put("config",config).toString()));if(response.has("error"))error(response.optString("error"))
                }
                "overlay"->{enableOverlay();if(!sessionActive)stopSelf()}
                "resumeProjection"->{grantProjection(intent);control("resume")}
                "pause","resume","stop"->control(intent.action!!)
            }
        }catch(e:Exception){warn(e.message?:"录音启动失败");NativeCore.command(JSONObject().put("op","fail").put("message",e.message?:"录音启动失败").toString());stopAudio();releaseProjection();recordingTypes=0;sessionActive=false;destroyOverlay();stopSelf()}
        return START_NOT_STICKY
    }
    private fun foreground(text:String,types:Int){
        val open=PendingIntent.getActivity(this,0,Intent(this,MainActivity::class.java),PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        val notification=Notification.Builder(this,"recording").setSmallIcon(com.lecsync.desk.R.drawable.ic_notification).setContentTitle(AppLanguages.text(this,"听译台")).setContentText(AppLanguages.text(this,text)).setContentIntent(open).setOngoing(true).setOnlyAlertOnce(true).build()
        startForeground(7,notification,types)
    }
    private fun grantProjection(intent:Intent){
        val data=if(Build.VERSION.SDK_INT>=33)intent.getParcelableExtra("projection",Intent::class.java)else @Suppress("DEPRECATION") intent.getParcelableExtra("projection")
        projection=getSystemService(MediaProjectionManager::class.java).getMediaProjection(intent.getIntExtra("resultCode",Activity.RESULT_CANCELED),data?:error("缺少系统音频授权"))
        projection?.registerCallback(object:MediaProjection.Callback(){override fun onStop(){if(projection!=null&&!stoppingProjection){projection=null;warn("系统撤销了音频捕获授权，采集已暂停；回到应用点击继续可重新授权。记录尚未结束。");control("pause")}}},handler)
    }
    fun needsProjection()=source!="mic"&&projection==null
    fun control(op:String){if(op=="pause"||op=="stop")stopAudio();NativeCore.command(JSONObject().put("op",op).toString());Bridge.broadcast()}
    private fun warn(message:String){NativeCore.command(JSONObject().put("op","warn").put("message",message).toString());Bridge.broadcast()}
    private val poll=object:Runnable{override fun run(){
        val state=JSONObject(NativeCore.command("{\"op\":\"snapshot\"}"));val p=state.optString("phase","idle")
        if(p=="recording"){if(wake?.isHeld==false)wake?.acquire()}else if(wake?.isHeld==true)wake?.release()
        val language=AppLanguages.resolve(this@SessionService,getSharedPreferences("desk",MODE_PRIVATE).getString("uiLanguage","system")?:"system")
        if(p!=previousPhase||language!=previousLanguage){previousPhase=p;previousLanguage=language
            if(p=="recording"&&!capturing)startAudio()
            if(p in listOf("paused","ended","stopping","pausing"))stopAudio()
            if(p=="ended"){releaseProjection();recordingTypes=0;sessionActive=false;destroyOverlay();Bridge.broadcast();stopSelf();return}
            else if(sessionActive)foreground(when(p){"recording"->"正在录音与翻译";"paused"->"已暂停 · 不上传音频";"connecting"->"正在连接";else->"正在处理"},recordingTypes)
        }
        refreshOverlay();Bridge.broadcast();handler.postDelayed(this,250)
    }}
    private fun buildAudio(playback:Boolean):AudioRecord {
        val format=AudioFormat.Builder().setEncoding(AudioFormat.ENCODING_PCM_16BIT).setSampleRate(16000).setChannelMask(AudioFormat.CHANNEL_IN_MONO).build()
        val minimum=AudioRecord.getMinBufferSize(16000,AudioFormat.CHANNEL_IN_MONO,AudioFormat.ENCODING_PCM_16BIT)
        check(minimum>0){"此设备不支持 16 kHz 音频采集"}
        val builder=AudioRecord.Builder().setAudioFormat(format).setBufferSizeInBytes(maxOf(minimum*4,6400))
        if(playback){val capture=AudioPlaybackCaptureConfiguration.Builder(projection?:error("系统音频授权失效")).addMatchingUsage(AudioAttributes.USAGE_MEDIA).addMatchingUsage(AudioAttributes.USAGE_GAME).addMatchingUsage(AudioAttributes.USAGE_UNKNOWN).excludeUid(android.os.Process.myUid()).build();builder.setAudioPlaybackCaptureConfig(capture)}else builder.setAudioSource(MediaRecorder.AudioSource.VOICE_RECOGNITION)
        return builder.build().apply{check(state==AudioRecord.STATE_INITIALIZED){"音频采集初始化失败"};if(!playback&&device!="default"){getSystemService(AudioManager::class.java).getDevices(AudioManager.GET_DEVICES_INPUTS).firstOrNull{it.id.toString()==device}?.let{setPreferredDevice(it)}}}
    }
    private fun startAudio(){capturing=true;val run=generation.incrementAndGet();audioWorker.execute{
        var m:AudioRecord?=null;var s:AudioRecord?=null
        try{
            if(source!="system")m=buildAudio(false)
            if(source!="mic")s=buildAudio(true)
            if(generation.get()!=run)return@execute
            mic=m;system=s;m?.startRecording();s?.startRecording()
            val mb=if(m!=null)ShortArray(320)else ShortArray(0);val sb=if(s!=null)ShortArray(320)else ShortArray(0)
            var silent=0;var warned=false
            fun read(record:AudioRecord?,buffer:ShortArray){if(record==null)return;var offset=0;while(offset<buffer.size&&generation.get()==run){val n=record.read(buffer,offset,buffer.size-offset,AudioRecord.READ_BLOCKING);check(n>0){"音源已停止或被系统占用"};offset+=n}}
            while(generation.get()==run){read(m,mb);read(s,sb);if(generation.get()!=run)break;if(s!=null){silent=if(sb.all{it==0.toShort()})silent+1 else 0;if(silent>=250&&!warned){warned=true;handler.post{warn("系统音轨暂时无声。请播放媒体，并确认来源应用允许音频捕获；通话和受保护内容可能无法录制。")}}};check(NativeCore.push(mb,sb,320)==0){"音频写入失败，请检查存储空间"}}
        }catch(e:Exception){if(generation.get()==run)handler.post{warn(e.message?:"音源采集失败");control("pause")}}
        finally{runCatching{m?.stop()};runCatching{s?.stop()};m?.release();s?.release();if(generation.get()==run){mic=null;system=null;capturing=false}}
    }}
    private fun stopAudio(){if(capturing){capturing=false;generation.incrementAndGet();runCatching{mic?.stop()};runCatching{system?.stop()};mic=null;system=null}}
    private fun releaseProjection(){stoppingProjection=true;projection?.stop();projection=null;stoppingProjection=false}
    fun refreshOverlay(){
        val enabled=SettingsStore.read(this).optBoolean("overlay")&&Settings.canDrawOverlays(this)
        val phase=JSONObject(NativeCore.command("{\"op\":\"snapshot\"}")).optString("phase")
        if(OverlayPolicy.shouldShow(sessionActive,phase,enabled,overlayDismissed,MainActivity.visible))showOverlay()else hideOverlay()
        if(!sessionActive)stopSelf()
    }
    fun enableOverlay(){overlayDismissed=false;refreshOverlay()}
    fun dismissOverlay(){overlayDismissed=true;hideOverlay()}
    fun setOverlayControlsVisible(visible:Boolean){overlayRoot?.controlsVisible=visible}
    fun overlayInteraction(active:Boolean){overlayBridge?.overlayInteraction(active)}
    fun setOverlayTextInput(enabled:Boolean){
        val root=overlayRoot?:return;val p=params?:return
        if(root.textInputActive==enabled)return
        root.textInputActive=enabled
        if(enabled){
            overlayOriginalSoftInputMode=p.softInputMode
            p.flags=p.flags and WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE.inv()
            p.softInputMode=WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE
        }else{
            p.flags=p.flags or WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE
            p.softInputMode=overlayOriginalSoftInputMode?:WindowManager.LayoutParams.SOFT_INPUT_STATE_UNSPECIFIED
            overlayOriginalSoftInputMode=null
        }
        applyOverlayLayout()
    }
    private fun prepareOverlay(){
            if(overlayView==null){
                val engine=FlutterEngine(this);overlayEngine=engine;overlayBridge=Bridge(this,engine);overlayPlatform=OverlayPlatform(this,engine)
                val loader=FlutterInjector.instance().flutterLoader();loader.startInitialization(this);loader.ensureInitializationComplete(this,null)
                val view=FlutterView(this,FlutterTextureView(this).apply{isOpaque=false});view.attachToFlutterEngine(engine)
                val root=OverlayFrame(this,this);root.addView(view,android.widget.FrameLayout.LayoutParams(-1,-1));overlayRoot=root;overlayView=view
                val d=resources.displayMetrics.density;val prefs=SettingsStore.read(this)
                params=WindowManager.LayoutParams((prefs.optDouble("overlayWidth",280.0)*d).toInt(),(prefs.optDouble("overlayHeight",180.0)*d).toInt(),WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY,WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE or WindowManager.LayoutParams.FLAG_NOT_TOUCH_MODAL,PixelFormat.TRANSLUCENT).apply{gravity=android.view.Gravity.TOP or android.view.Gravity.LEFT;x=(prefs.optDouble("overlayX",12.0)*d).toInt();y=(prefs.optDouble("overlayY",90.0)*d).toInt()}
                engine.dartExecutor.executeDartEntrypoint(DartExecutor.DartEntrypoint(loader.findAppBundlePath(),"overlayMain"))
            }
    }
    private fun showOverlay(){
        if(overlayAdded)return
        try{
            prepareOverlay()
            normalizeOverlayGeometry()
            overlayAdded=true
            getSystemService(WindowManager::class.java).addView(overlayRoot,params);overlayEngine?.lifecycleChannel?.appIsResumed();overlayBridge?.overlayShown()
        }catch(e:Exception){destroyOverlay();warn("悬浮窗显示失败，请检查悬浮窗权限")}
    }
    private fun screenBounds():android.graphics.Rect {
        val wm=getSystemService(WindowManager::class.java)
        return if(Build.VERSION.SDK_INT>=30){
            val metrics=wm.currentWindowMetrics
            val insets=metrics.windowInsets.getInsetsIgnoringVisibility(android.view.WindowInsets.Type.systemBars() or android.view.WindowInsets.Type.displayCutout())
            android.graphics.Rect(0,0,metrics.bounds.width()-insets.left-insets.right,metrics.bounds.height()-insets.top-insets.bottom)
        }else android.graphics.Rect(0,0,resources.displayMetrics.widthPixels,resources.displayMetrics.heightPixels)
    }
    fun overlayGeometry():IntArray {val p=params?:return intArrayOf(0,0,0,0);return intArrayOf(p.x,p.y,p.width,p.height)}
    private fun normalizeOverlayGeometry(){
        if(params==null)return
        val geometry=expandedGeometry?:overlayGeometry()
        setOverlayGeometry(geometry[0],geometry[1],geometry[2],geometry[3])
        if(dockSide!=null){expandedGeometry=overlayGeometry();applyDockedGeometry()}
    }
    private fun applyDockedGeometry(){
        val p=params?:return;val side=dockSide?:return;val expanded=expandedGeometry?:return
        val geometry=OverlayDock.tuckedGeometry(expanded,side,screenBounds().width(),(16*resources.displayMetrics.density).toInt())
        p.x=geometry[0];p.y=geometry[1];p.width=geometry[2];p.height=geometry[3]
        p.flags=p.flags or WindowManager.LayoutParams.FLAG_LAYOUT_NO_LIMITS
        overlayRoot?.setDocked(side)
    }
    internal fun finishOverlayDrag(origin:IntArray,dx:Float,dy:Float){
        if(dockSide!=null || params==null)return
        val geometry=overlayGeometry()
        val side=OverlayDock.confirmedSideForDrag(origin,geometry,screenBounds().width(),dx,dy,
            24*resources.displayMetrics.density)?:return
        dockSide=side;expandedGeometry=geometry.copyOf();applyDockedGeometry();applyOverlayLayout()
    }
    fun restoreOverlayFromDock(){
        val p=params?:return;val geometry=expandedGeometry?:return
        dockSide=null;expandedGeometry=null
        p.flags=p.flags and WindowManager.LayoutParams.FLAG_LAYOUT_NO_LIMITS.inv()
        overlayRoot?.setDocked(null)
        setOverlayGeometry(geometry[0],geometry[1],geometry[2],geometry[3])
        applyOverlayLayout();rememberOverlayGeometry();overlayBridge?.overlayShown()
    }
    fun setOverlayGeometry(x:Int,y:Int,width:Int,height:Int){
        val p=params?:return;val bounds=screenBounds();val d=resources.displayMetrics.density
        val maxW=maxOf(1,bounds.width()-(16*d).toInt());val maxH=maxOf(1,bounds.height()-(24*d).toInt())
        p.width=width.coerceIn(min((200*d).toInt(),maxW),maxW);p.height=height.coerceIn(min((120*d).toInt(),maxH),maxH)
        p.x=x.coerceIn(0,maxOf(0,bounds.width()-p.width));p.y=y.coerceIn(0,maxOf(0,bounds.height()-p.height))
    }
    fun scaleOverlay(factor:Float){val p=params?:return;setOverlayGeometry(p.x,p.y,(p.width*factor).toInt(),(p.height*factor).toInt())}
    internal fun resizeOverlay(origin:IntArray,corner:OverlayCorner,dx:Int,dy:Int){
        val bounds=screenBounds();val d=resources.displayMetrics.density
        val geometry=OverlayResize.resize(origin,corner,dx,dy,bounds.width(),bounds.height(),
            (200*d).toInt(),(120*d).toInt(),maxOf(1,bounds.width()-(16*d).toInt()),maxOf(1,bounds.height()-(24*d).toInt()))
        setOverlayGeometry(geometry[0],geometry[1],geometry[2],geometry[3])
    }
    fun toggleOverlaySize(){val p=params?:return;val d=resources.displayMetrics.density;val expanded=min((560*d).toInt(),screenBounds().width()-(16*d).toInt());val large=p.width>=expanded*.8;setOverlayGeometry(p.x,p.y,if(large)(280*d).toInt()else expanded,((if(large)180 else 300)*d).toInt());rememberOverlayGeometry()}
    fun applyOverlayLayout(){if(overlayAdded)getSystemService(WindowManager::class.java).updateViewLayout(overlayRoot,params)}
    fun rememberOverlayGeometry(){if(params==null)return;val geometry=expandedGeometry?:overlayGeometry();val d=resources.displayMetrics.density;SettingsStore.save(this,JSONObject().put("overlayX",geometry[0]/d).put("overlayY",geometry[1]/d).put("overlayWidth",geometry[2]/d).put("overlayHeight",geometry[3]/d))}
    private fun hideOverlay(){
        if(!overlayAdded)return
        overlayAdded=false
        rememberOverlayGeometry()
        runCatching{getSystemService(WindowManager::class.java).removeViewImmediate(overlayRoot)}
        overlayEngine?.lifecycleChannel?.appIsPaused()
    }
    private fun destroyOverlay(){hideOverlay();overlayView?.detachFromFlutterEngine();overlayView=null;overlayRoot=null;params=null;overlayOriginalSoftInputMode=null;dockSide=null;expandedGeometry=null;overlayBridge?.close();overlayBridge=null;overlayPlatform?.close();overlayPlatform=null;overlayEngine?.destroy();overlayEngine=null}
    override fun onConfigurationChanged(newConfig:Configuration){super.onConfigurationChanged(newConfig);if(params!=null){normalizeOverlayGeometry();applyOverlayLayout()}}
    override fun onTaskRemoved(rootIntent:Intent?){refreshOverlay();super.onTaskRemoved(rootIntent)}
    override fun onDestroy(){handler.removeCallbacksAndMessages(null);stopAudio();if(wake?.isHeld==true)wake?.release();NativeCore.command("{\"op\":\"stop\"}");releaseProjection();destroyOverlay();audioWorker.shutdown();instance=null;super.onDestroy()}
    companion object {@Volatile var instance:SessionService?=null}
}

