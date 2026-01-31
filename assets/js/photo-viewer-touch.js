/**
 * Touch gesture handling for PhotoViewer
 * Supports swipe navigation, pinch-to-zoom, and double-tap zoom
 */
function setupTouchGestures(viewer) {
    const container = viewer.mediaContainerWrapper;
    let touchStartX = 0;
    let touchStartY = 0;
    let isPinching = false;
    let initialPinchDistance = 0;
    let lastTap = 0;

    function getTouchDistance(touches) {
        const dx = touches[0].clientX - touches[1].clientX;
        const dy = touches[0].clientY - touches[1].clientY;
        return Math.hypot(dx, dy);
    }

    container.addEventListener('touchstart', (e) => {
        if (e.touches.length === 1) {
            touchStartX = e.touches[0].clientX;
            touchStartY = e.touches[0].clientY;
        } else if (e.touches.length === 2) {
            isPinching = true;
            initialPinchDistance = getTouchDistance(e.touches);
        }
    }, { passive: true });

    container.addEventListener('touchmove', (e) => {
        if (e.touches.length === 2 && isPinching) {
            const currentDistance = getTouchDistance(e.touches);
            const scale = (currentDistance / initialPinchDistance) * viewer.currentScale;
            viewer.applyZoom(Math.max(1, Math.min(3, scale)));
        }
    }, { passive: true });

    container.addEventListener('touchend', (e) => {
        if (isPinching) {
            isPinching = false;
            const img = viewer.mediaContainer.querySelector('img, video');
            if (img) {
                const match = img.style.transform.match(/scale\(([\d.]+)\)/);
                if (match) {
                    viewer.currentScale = parseFloat(match[1]);
                }
                if (viewer.currentScale < 1.1) {
                    viewer.resetZoom();
                }
            }
            return;
        }

        // Swipe navigation (only if not zoomed)
        if (viewer.currentScale > 1.05) return;

        const touchEndX = e.changedTouches[0].clientX;
        const touchEndY = e.changedTouches[0].clientY;
        const diffX = touchEndX - touchStartX;
        const diffY = touchEndY - touchStartY;

        if (Math.abs(diffX) > Math.abs(diffY) && Math.abs(diffX) > 50) {
            viewer.navigate(diffX > 0 ? -1 : 1);
        }
    }, { passive: true });

    // Double tap to zoom
    container.addEventListener('touchend', (e) => {
        if (e.touches.length > 0) return;

        const now = Date.now();
        if (now - lastTap < 300) {
            if (viewer.currentScale > 1.05) {
                viewer.resetZoom();
            } else {
                viewer.applyZoom(2);
                viewer.currentScale = 2;
            }
        }
        lastTap = now;
    }, { passive: true });
}