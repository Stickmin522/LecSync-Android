package com.lecsync.desk

import android.content.Context
import android.graphics.Color
import android.graphics.drawable.GradientDrawable
import android.view.Choreographer
import android.view.Gravity
import android.view.MotionEvent
import android.view.ScaleGestureDetector
import android.view.View
import android.view.ViewConfiguration
import android.widget.FrameLayout

/** Native window gestures avoid Flutter/JNI round trips during each move. */
class OverlayFrame(context: Context, private val service: SessionService) : FrameLayout(context) {
    var controlsVisible = true
    private var mode = 0
    private var startX = 0f
    private var startY = 0f
    private var origin = IntArray(4)
    private var resizeCorner: OverlayCorner? = null
    private var scheduled = false
    private var lastTap = 0L
    private var lastTapX = 0f
    private var lastTapY = 0f
    private val density = resources.displayMetrics.density
    private var docked = false
    internal var textInputActive = false
        set(value) {
            field = value
            mode = 0
            resizeCorner = null
            lastTap = 0L
            if (value) service.overlayInteraction(false)
        }
    private val dockHandle = View(context).apply {
        background = GradientDrawable().apply {
            setColor(Color.rgb(36, 92, 232)); cornerRadius = 8*density
        }
        contentDescription = AppLanguages.text(context,"悬浮字幕")
        setOnClickListener { service.restoreOverlayFromDock() }
        visibility = View.GONE
    }
    init { addView(dockHandle, LayoutParams((16*density).toInt(), LayoutParams.MATCH_PARENT)) }

    internal fun setDocked(side: OverlayDockSide?) {
        docked = side != null
        for (i in 0 until childCount) {
            val child = getChildAt(i)
            if (child !== dockHandle) child.visibility = if (docked) View.INVISIBLE else View.VISIBLE
        }
        dockHandle.layoutParams = LayoutParams((16*density).toInt(), LayoutParams.MATCH_PARENT,
            if(side == OverlayDockSide.LEFT) Gravity.RIGHT else Gravity.LEFT)
        dockHandle.visibility = if(docked) View.VISIBLE else View.GONE
    }
    private val pinch = ScaleGestureDetector(context, object : ScaleGestureDetector.SimpleOnScaleGestureListener() {
        override fun onScale(detector: ScaleGestureDetector): Boolean {
            service.scaleOverlay(detector.scaleFactor); schedule(); return true
        }
    })
    private fun schedule() {
        if (scheduled) return
        scheduled = true
        Choreographer.getInstance().postFrameCallback { scheduled=false; service.applyOverlayLayout() }
    }
    override fun dispatchTouchEvent(event: MotionEvent): Boolean {
        // Text menus, selection and the editor belong to Flutter while the window
        // is focusable. Their touches must not resize, move or collapse the window.
        if (textInputActive) return super.dispatchTouchEvent(event)
        if (event.actionMasked == MotionEvent.ACTION_DOWN) {
            // Keep the corner hit areas clear of the return/close button centers.
            resizeCorner = OverlayResize.cornerAt(event.x, event.y, width, height, 20*density)
            val header = controlsVisible && event.y <= 46*density && event.x < width-94*density
            // A hidden toolbar's first touch only reveals it, avoiding accidental button activation.
            mode=when { docked->5;resizeCorner!=null->2;header->1;!controlsVisible->4;else->0 }
            startX=event.rawX;startY=event.rawY;origin=service.overlayGeometry()
            service.overlayInteraction(true)
        }
        if (mode!=5 && event.actionMasked == MotionEvent.ACTION_POINTER_DOWN && event.pointerCount>=2) {
            if(mode==0){val cancel=MotionEvent.obtain(event);cancel.action=MotionEvent.ACTION_CANCEL;super.dispatchTouchEvent(cancel);cancel.recycle()}
            mode=3
        }
        if(mode==3){pinch.onTouchEvent(event)}
        if(event.actionMasked==MotionEvent.ACTION_MOVE){
            when(mode){
                1->service.setOverlayGeometry(origin[0]+(event.rawX-startX).toInt(),origin[1]+(event.rawY-startY).toInt(),origin[2],origin[3])
                2->resizeCorner?.let { service.resizeOverlay(origin,it,(event.rawX-startX).toInt(),(event.rawY-startY).toInt()) }
            }
            if(mode!=0)schedule()
        }
        val consume=mode!=0
        if(event.actionMasked==MotionEvent.ACTION_UP || event.actionMasked==MotionEvent.ACTION_CANCEL){
            val tapped=kotlin.math.abs(event.rawX-startX)<ViewConfiguration.get(context).scaledTouchSlop &&
                kotlin.math.abs(event.rawY-startY)<ViewConfiguration.get(context).scaledTouchSlop
            if(mode==5 && tapped && event.actionMasked==MotionEvent.ACTION_UP)service.restoreOverlayFromDock()
            if(mode==1 && !tapped && event.actionMasked==MotionEvent.ACTION_UP){
                service.finishOverlayDrag(origin,event.rawX-startX,event.rawY-startY)
            }
            if(mode==1 && event.actionMasked==MotionEvent.ACTION_UP && tapped){
                if(lastTap>0 && event.eventTime-lastTap<=ViewConfiguration.getDoubleTapTimeout() &&
                    kotlin.math.abs(event.rawX-lastTapX)<40*density && kotlin.math.abs(event.rawY-lastTapY)<40*density){
                    service.toggleOverlaySize();lastTap=0L
                }else{lastTap=event.eventTime;lastTapX=event.rawX;lastTapY=event.rawY}
            } else {lastTap=0L}
            if(consume){service.applyOverlayLayout();service.rememberOverlayGeometry()};mode=0;resizeCorner=null
            service.overlayInteraction(false)
        }
        return if(consume)true else super.dispatchTouchEvent(event)
    }
}
