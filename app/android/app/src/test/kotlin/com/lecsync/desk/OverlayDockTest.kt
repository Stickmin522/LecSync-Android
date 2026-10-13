package com.lecsync.desk

import org.junit.Assert.*
import org.junit.Test

class OverlayDockTest {
    @Test fun detectsBothPhysicalEdges() {
        assertEquals(OverlayDockSide.LEFT,OverlayDock.sideFor(intArrayOf(0,90,280,180),800,12))
        assertEquals(OverlayDockSide.LEFT,OverlayDock.sideFor(intArrayOf(12,90,280,180),800,12))
        assertEquals(OverlayDockSide.RIGHT,OverlayDock.sideFor(intArrayOf(520,90,280,180),800,12))
        assertEquals(OverlayDockSide.RIGHT,OverlayDock.sideFor(intArrayOf(508,90,280,180),800,12))
        assertNull(OverlayDock.sideFor(intArrayOf(250,90,280,180),800,12))
    }

    @Test fun firstDragToEitherEdgeDoesNotCollapseEvenWithLargeOutwardMovement() {
        val origin=intArrayOf(250,90,280,180)
        assertNull(OverlayDock.confirmedSideForDrag(origin,intArrayOf(0,90,280,180),800,-500f,0f,24f))
        assertNull(OverlayDock.confirmedSideForDrag(origin,intArrayOf(520,90,280,180),800,500f,0f,24f))
    }

    @Test fun newOutwardDragFromTheEdgeConfirmsCollapseOnBothSides() {
        val left=intArrayOf(0,90,280,180)
        val right=intArrayOf(520,90,280,180)
        assertEquals(OverlayDockSide.LEFT,OverlayDock.confirmedSideForDrag(left,left,800,-24f,0f,24f))
        assertEquals(OverlayDockSide.RIGHT,OverlayDock.confirmedSideForDrag(right,right,800,24f,0f,24f))
    }

    @Test fun nearEdgeStartDoesNotCountAsTheConfirmationDrag() {
        assertNull(OverlayDock.confirmedSideForDrag(intArrayOf(12,90,280,180),intArrayOf(0,90,280,180),800,-100f,0f,24f))
        assertNull(OverlayDock.confirmedSideForDrag(intArrayOf(508,90,280,180),intArrayOf(520,90,280,180),800,100f,0f,24f))
    }

    @Test fun smallMotionInwardMotionAndVerticalDragsKeepTheWindowExpanded() {
        val left=intArrayOf(0,90,280,180)
        val right=intArrayOf(520,90,280,180)
        for((geometry,direction) in listOf(left to -1,right to 1)){
            assertNull(OverlayDock.confirmedSideForDrag(geometry,geometry,800,23f*direction,0f,24f))
            assertNull(OverlayDock.confirmedSideForDrag(geometry,geometry,800,-30f*direction,0f,24f))
            assertNull(OverlayDock.confirmedSideForDrag(geometry,geometry,800,30f*direction,31f,24f))
            assertNull(OverlayDock.confirmedSideForDrag(geometry,geometry,800,30f*direction,-30f,24f))
        }
    }

    @Test fun leavingTheStartingEdgeOrReachingTheOppositeEdgeDoesNotConfirmCollapse() {
        val left=intArrayOf(0,90,280,180)
        val right=intArrayOf(520,90,280,180)
        assertNull(OverlayDock.confirmedSideForDrag(left,intArrayOf(80,90,280,180),800,-30f,0f,24f))
        assertNull(OverlayDock.confirmedSideForDrag(left,right,800,520f,0f,24f))
        assertNull(OverlayDock.confirmedSideForDrag(right,left,800,-520f,0f,24f))
    }

    @Test fun bothSidesLeaveOnlyTheTabOnScreenAndKeepTheExpandedGeometry() {
        for(side in OverlayDockSide.entries){
            val expanded=intArrayOf(if(side==OverlayDockSide.LEFT)0 else 520,90,280,180)
            val saved=expanded.copyOf()
            val tucked=OverlayDock.tuckedGeometry(expanded,side,800,16)
            val visible=minOf(800,tucked[0]+tucked[2])-maxOf(0,tucked[0])
            assertEquals(16,visible)
            assertArrayEquals(saved,expanded)
            assertEquals(90,tucked[1]);assertEquals(280,tucked[2]);assertEquals(180,tucked[3])
        }
    }

    @Test fun tabsStayReachableAfterChangingScreenWidth() {
        val expanded=intArrayOf(520,90,280,180)
        val right=OverlayDock.tuckedGeometry(expanded,OverlayDockSide.RIGHT,400,16)
        assertEquals(384,right[0])
        val left=OverlayDock.tuckedGeometry(expanded,OverlayDockSide.LEFT,400,16)
        assertEquals(-264,left[0])
    }
}
