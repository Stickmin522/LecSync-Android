package com.lecsync.desk

import kotlin.math.abs

internal enum class OverlayDockSide { LEFT, RIGHT }

internal object OverlayDock {
    fun sideFor(geometry: IntArray, screenWidth: Int, threshold: Int): OverlayDockSide? = when {
        geometry[0] <= threshold -> OverlayDockSide.LEFT
        screenWidth - geometry[0] - geometry[2] <= threshold -> OverlayDockSide.RIGHT
        else -> null
    }

    /** Docking needs a new outward drag after the expanded window has reached the edge. */
    fun confirmedSideForDrag(
        origin: IntArray,
        current: IntArray,
        screenWidth: Int,
        dx: Float,
        dy: Float,
        confirmationDistance: Float
    ): OverlayDockSide? {
        // Use the geometry captured at ACTION_DOWN, so one long drag from the middle
        // cannot count both as reaching the edge and as confirming the collapse.
        val side = sideFor(origin, screenWidth, 0) ?: return null
        if (sideFor(current, screenWidth, 0) != side) return null
        val outward = if (side == OverlayDockSide.LEFT) -dx else dx
        if (outward < confirmationDistance || outward <= abs(dy)) return null
        return side
    }

    fun tuckedGeometry(expanded: IntArray, side: OverlayDockSide, screenWidth: Int, stripWidth: Int): IntArray {
        val strip = stripWidth.coerceIn(1, expanded[2])
        return intArrayOf(
            if (side == OverlayDockSide.LEFT) strip - expanded[2] else screenWidth - strip,
            expanded[1], expanded[2], expanded[3]
        )
    }
}
