/**
 * Touch gesture handling for PhotoViewer
 * Supports swipe navigation, pinch-to-zoom, pan when zoomed, and double-tap zoom
 */
function setupTouchGestures(viewer) {
    const container = viewer.mediaContainerWrapper;
    let touchStartX = 0;
    let touchStartY = 0;
    let isPinching = false;
    let isPanning = false;
    let initialPinchDistance = 0;
    let initialScale = 1;
    let initialTranslateX = 0;
    let initialTranslateY = 0;
    let pinchCenterX = 0;
    let pinchCenterY = 0;
    let lastTap = 0;

    function getTouchDistance(touches) {
        const dx = touches[0].clientX - touches[1].clientX;
        const dy = touches[0].clientY - touches[1].clientY;
        return Math.hypot(dx, dy);
    }

    function getTouchCenter(touches) {
        return {
            x: (touches[0].clientX + touches[1].clientX) / 2,
            y: (touches[0].clientY + touches[1].clientY) / 2
        };
    }

    function getImageRect() {
        const img = viewer.mediaContainer.querySelector('img, video');
        if (!img) return null;
        return img.getBoundingClientRect();
    }

    function constrainTranslation() {
        const img = viewer.mediaContainer.querySelector('img, video');
        if (!img || viewer.currentScale <= 1) return;

        const containerRect = container.getBoundingClientRect();
        const imgRect = img.getBoundingClientRect();

        // Calculate how much the scaled image extends beyond the container
        const extraWidth = (imgRect.width - containerRect.width) / 2;
        const extraHeight = (imgRect.height - containerRect.height) / 2;

        if (extraWidth > 0) {
            viewer.translateX = Math.max(-extraWidth, Math.min(extraWidth, viewer.translateX));
        } else {
            viewer.translateX = 0;
        }

        if (extraHeight > 0) {
            viewer.translateY = Math.max(-extraHeight, Math.min(extraHeight, viewer.translateY));
        } else {
            viewer.translateY = 0;
        }
    }

    container.addEventListener('touchstart', (e) => {
        if (e.touches.length === 1) {
            touchStartX = e.touches[0].clientX;
            touchStartY = e.touches[0].clientY;
            initialTranslateX = viewer.translateX;
            initialTranslateY = viewer.translateY;
            isPanning = viewer.currentScale > 1.05;
        } else if (e.touches.length === 2) {
            isPinching = true;
            isPanning = false;
            initialPinchDistance = getTouchDistance(e.touches);
            initialScale = viewer.currentScale;
            initialTranslateX = viewer.translateX;
            initialTranslateY = viewer.translateY;
            const center = getTouchCenter(e.touches);
            pinchCenterX = center.x;
            pinchCenterY = center.y;
        }
    }, { passive: true });

    container.addEventListener('touchmove', (e) => {
        if (e.touches.length === 2 && isPinching) {
            const currentDistance = getTouchDistance(e.touches);
            const newScale = Math.max(1, Math.min(3, (currentDistance / initialPinchDistance) * initialScale));
            const scaleChange = newScale / initialScale;

            // Get container center
            const containerRect = container.getBoundingClientRect();
            const containerCenterX = containerRect.left + containerRect.width / 2;
            const containerCenterY = containerRect.top + containerRect.height / 2;

            // Calculate offset from pinch center to container center
            const offsetX = pinchCenterX - containerCenterX;
            const offsetY = pinchCenterY - containerCenterY;

            // Adjust translation to zoom towards pinch center
            viewer.translateX = initialTranslateX - offsetX * (scaleChange - 1);
            viewer.translateY = initialTranslateY - offsetY * (scaleChange - 1);
            viewer.currentScale = newScale;

            constrainTranslation();
            viewer.applyTransform();
        } else if (e.touches.length === 1 && isPanning && viewer.currentScale > 1.05) {
            const dx = e.touches[0].clientX - touchStartX;
            const dy = e.touches[0].clientY - touchStartY;
            viewer.translateX = initialTranslateX + dx;
            viewer.translateY = initialTranslateY + dy;
            constrainTranslation();
            viewer.applyTransform();
        }
    }, { passive: true });

    container.addEventListener('touchend', (e) => {
        if (isPinching) {
            isPinching = false;
            if (viewer.currentScale < 1.1) {
                viewer.resetZoom();
            }
            return;
        }

        if (isPanning) {
            isPanning = false;
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
                viewer.currentScale = 2;
                viewer.applyTransform();
            }
        }
        lastTap = now;
    }, { passive: true });
}