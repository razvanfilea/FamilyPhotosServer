/**
 * Animation handling for PhotoViewer
 * Provides shared element transitions between thumbnail and fullscreen view
 */

const HORIZONTAL_PADDING = 16;
const VERTICAL_PADDING = 80;

function animateViewerOpen(viewer) {
    if (!viewer.thumbnailElement || !viewer.openThumbRect) {
        viewer.animationComplete = true;
        tryFinishViewerTransition(viewer);
        return;
    }

    const thumbRect = viewer.openThumbRect;
    const thumbImg = viewer.thumbnailElement.querySelector('img');

    viewer.animClone = document.createElement('div');
    viewer.animClone.className = 'photo-viewer-anim-clone';
    viewer.animClone.style.cssText = `
        position: fixed;
        left: ${thumbRect.left}px;
        top: ${thumbRect.top}px;
        width: ${thumbRect.width}px;
        height: ${thumbRect.height}px;
        z-index: 1001;
        overflow: hidden;
        border-radius: 0;
        background: #000;
    `;

    if (thumbImg) {
        const imgClone = thumbImg.cloneNode(true);
        imgClone.style.cssText = 'width: 100%; height: 100%; object-fit: cover;';
        viewer.animClone.appendChild(imgClone);
    }

    document.body.appendChild(viewer.animClone);
    viewer.mediaContainer.style.opacity = '0';

    const finalWidth = window.innerWidth - (HORIZONTAL_PADDING * 2);
    const finalHeight = window.innerHeight - (VERTICAL_PADDING * 2);

    requestAnimationFrame(() => {
        requestAnimationFrame(() => {
            if (!viewer.animClone) return;

            viewer.animClone.style.transition = 'all 0.3s cubic-bezier(0.4, 0, 0.2, 1)';
            viewer.animClone.style.left = `${HORIZONTAL_PADDING}px`;
            viewer.animClone.style.top = `${VERTICAL_PADDING}px`;
            viewer.animClone.style.width = `${finalWidth}px`;
            viewer.animClone.style.height = `${finalHeight}px`;
            viewer.animClone.style.borderRadius = '8px';

            const cloneImg = viewer.animClone.querySelector('img');
            if (cloneImg) {
                cloneImg.style.transition = 'object-fit 0.3s';
                cloneImg.style.objectFit = 'contain';
            }

            setTimeout(() => {
                viewer.animationComplete = true;
                tryFinishViewerTransition(viewer);
            }, 300);
        });
    });
}

function tryFinishViewerTransition(viewer) {
    if (!viewer.animationComplete || !viewer.contentReady) return;

    if (viewer.animClone) {
        viewer.animClone.style.transition = 'opacity 0.15s ease';
        viewer.animClone.style.opacity = '0';
        viewer.mediaContainer.style.transition = 'opacity 0.15s ease';
        viewer.mediaContainer.style.opacity = '1';

        setTimeout(() => {
            if (viewer.animClone) {
                viewer.animClone.remove();
                viewer.animClone = null;
            }
        }, 150);
    } else {
        viewer.mediaContainer.style.opacity = '1';
    }
}

function animateViewerClose(viewer) {
    const media = viewer.mediaContainer;
    const thumb = viewer.thumbnailElement;
    const thumbRect = thumb ? thumb.getBoundingClientRect() : null;

    const isThumbVisible = thumbRect &&
        thumbRect.top < window.innerHeight &&
        thumbRect.bottom > 0 &&
        thumbRect.left < window.innerWidth &&
        thumbRect.right > 0;

    if (thumb && isThumbVisible) {
        const startWidth = window.innerWidth - (HORIZONTAL_PADDING * 2);
        const startHeight = window.innerHeight - (VERTICAL_PADDING * 2);
        const currentImg = media.querySelector('img, video');

        viewer.animClone = document.createElement('div');
        viewer.animClone.className = 'photo-viewer-anim-clone';
        viewer.animClone.style.cssText = `
            position: fixed;
            left: ${HORIZONTAL_PADDING}px;
            top: ${VERTICAL_PADDING}px;
            width: ${startWidth}px;
            height: ${startHeight}px;
            z-index: 1001;
            overflow: hidden;
            border-radius: 8px;
            background: #000;
        `;

        if (currentImg) {
            const imgClone = currentImg.cloneNode(true);
            imgClone.style.cssText = 'width: 100%; height: 100%; object-fit: contain; transition: object-fit 0.25s;';
            viewer.animClone.appendChild(imgClone);
        }

        document.body.appendChild(viewer.animClone);
        viewer.container.classList.add('hidden');
        media.style = '';
        media.innerHTML = '';

        requestAnimationFrame(() => {
            requestAnimationFrame(() => {
                if (!viewer.animClone) return;

                viewer.animClone.style.transition = 'all 0.25s cubic-bezier(0.4, 0, 0.2, 1)';
                const cloneImg = viewer.animClone.querySelector('img, video');
                if (cloneImg) cloneImg.style.objectFit = 'cover';

                viewer.animClone.style.left = `${thumbRect.left}px`;
                viewer.animClone.style.top = `${thumbRect.top}px`;
                viewer.animClone.style.width = `${thumbRect.width}px`;
                viewer.animClone.style.height = `${thumbRect.height}px`;
                viewer.animClone.style.opacity = '0';
                viewer.animClone.style.borderRadius = '0';

                setTimeout(() => {
                    if (viewer.animClone) {
                        viewer.animClone.remove();
                        viewer.animClone = null;
                    }
                }, 250);
            });
        });
        return;
    }

    viewer.container.classList.add('hidden');
    media.style = '';
    media.innerHTML = '';
}
