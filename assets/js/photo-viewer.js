class PhotoViewer {
    constructor() {
        this.container = null;
        this.currentPhotoId = null;
        this.currentPhotoIndex = -1;
        this.photoList = [];
        this.thumbnailElement = null;
        this.isOpen = false;
        this.infoPanelOpen = false;

        // Animation state
        this.animClone = null;
        this.openThumbRect = null;
        this.contentReady = false;
        this.animationComplete = false;

        // Zoom state
        this.currentScale = 1;

        this.createElements();
        this.bindEvents();
    }

    createElements() {
        const template = document.getElementById('photo-viewer-template');
        this.container = template.content.firstElementChild.cloneNode(true);
        document.body.appendChild(this.container);

        // Cache element references
        this.backdrop = this.container.querySelector('.photo-viewer-backdrop');
        this.mediaContainer = this.container.querySelector('.photo-viewer-media');
        this.mediaContainerWrapper = this.container.querySelector('.photo-viewer-media-container');
        this.closeBtn = this.container.querySelector('.photo-viewer-close');
        this.prevBtn = this.container.querySelector('.photo-viewer-nav-prev');
        this.nextBtn = this.container.querySelector('.photo-viewer-nav-next');
        this.infoPanel = this.container.querySelector('#photo-info-panel');
        this.actionsContainer = this.container.querySelector('#viewer-actions-container');
    }

    bindEvents() {
        // Close handlers
        this.backdrop.addEventListener('click', () => this.close());
        this.closeBtn.addEventListener('click', () => this.close());

        // Navigation
        this.prevBtn.addEventListener('click', () => this.navigate(-1));
        this.nextBtn.addEventListener('click', () => this.navigate(1));

        // Keyboard navigation
        document.addEventListener('keydown', (e) => this.handleKeydown(e));

        // Handle browser back button on mobile
        window.addEventListener('popstate', () => {
            if (this.isOpen) {
                this.close(true);
            }
        });

        // Info panel close
        const infoPanelClose = this.container.querySelector('#photo-info-close');
        if (infoPanelClose) {
            infoPanelClose.addEventListener('click', () => this.toggleInfoPanel(false));
        }

        // Touch gestures (from photo-viewer-touch.js)
        setupTouchGestures(this);

        // HTMX event handlers
        this.bindHtmxEvents();
    }

    bindHtmxEvents() {
        // Open info panel when content is loading
        document.body.addEventListener('htmx:beforeRequest', (e) => {
            if (e.detail.target?.id === 'photo-info-content') {
                this.infoPanelOpen = true;
                this.infoPanel.classList.add('open');
            }
        });

        // Handle post-delete navigation
        document.body.addEventListener('htmx:afterRequest', (e) => {
            const path = e.detail.pathInfo?.requestPath;
            if (path?.startsWith('/trash/') && e.detail.successful) {
                this.showToast('Photo moved to trash');
                this.handlePhotoRemoval();
            }
        });

        // Handle media load after HTMX swap
        document.body.addEventListener('htmx:afterSwap', (e) => {
            if (e.detail.target === this.mediaContainer) {
                const media = this.mediaContainer.querySelector('img, video');
                if (media) {
                    const onReady = () => {
                        this.contentReady = true;
                        tryFinishViewerTransition(this);
                    };
                    if (media.tagName === 'VIDEO') {
                        media.oncanplay = onReady;
                        setTimeout(onReady, 1000);
                    } else if (media.complete) {
                        onReady();
                    } else {
                        media.onload = onReady;
                        setTimeout(onReady, 2000);
                    }
                }
                // Re-bind share button (not handled by HTMX)
                document.getElementById('viewer-share-btn')?.addEventListener('click', () => this.sharePhoto());
            }
        });
    }

    applyZoom(scale) {
        const img = this.mediaContainer.querySelector('img, video');
        if (img) {
            img.style.transform = `scale(${scale})`;
        }
    }

    resetZoom() {
        this.currentScale = 1;
        const img = this.mediaContainer.querySelector('img, video');
        if (img) {
            img.style.transform = 'scale(1)';
        }
    }

    // ==================== Photo List Management ====================

    buildPhotoList() {
        this.photoList = Array.from(document.querySelectorAll('.photo-card[data-photo-id]'))
            .map(el => ({
                id: parseInt(el.dataset.photoId, 10),
                element: el
            }));
    }

    // ==================== Open/Close ====================

    open(photoId, thumbnailElement) {
        this.buildPhotoList();
        this.currentPhotoId = photoId;
        this.thumbnailElement = thumbnailElement;
        this.currentPhotoIndex = this.photoList.findIndex(p => p.id === photoId);
        this.openThumbRect = thumbnailElement ? thumbnailElement.getBoundingClientRect() : null;

        this.container.setAttribute('data-photo-id', photoId);
        this.container.classList.remove('hidden');
        this.isOpen = true;
        document.body.style.overflow = 'hidden';
        history.pushState({ photoViewer: true }, '');

        this.contentReady = false;
        this.animationComplete = false;
        this.currentScale = 1;

        animateViewerOpen(this);
        this.loadPhoto(photoId);
        this.updateNavigation();
    }

    close(fromPopState = false) {
        if (!this.isOpen) return;

        this.isOpen = false;
        this.infoPanelOpen = false;
        this.infoPanel.classList.remove('open');
        this.resetZoom();
        animateViewerClose(this);
        document.body.style.overflow = '';

        // Go back in history unless we're already handling popstate
        if (!fromPopState && history.state?.photoViewer) {
            history.back();
        }
    }

    // ==================== Navigation ====================

    navigate(direction) {
        const newIndex = this.currentPhotoIndex + direction;
        if (newIndex < 0 || newIndex >= this.photoList.length) return;

        this.currentPhotoIndex = newIndex;
        const photo = this.photoList[newIndex];
        this.currentPhotoId = photo.id;
        this.thumbnailElement = photo.element;

        // Update container state
        this.container.setAttribute('data-photo-id', photo.id);

        // Reset zoom when navigating
        this.resetZoom();

        this.loadPhoto(photo.id);
        this.updateNavigation();

        // Update info panel if open (HTMX handles the request)
        if (this.infoPanelOpen) {
            document.getElementById('viewer-info-btn')?.click();
        }
    }

    updateNavigation() {
        if (this.prevBtn) {
            this.prevBtn.style.display = this.currentPhotoIndex > 0 ? '' : 'none';
        }
        if (this.nextBtn) {
            this.nextBtn.style.display = this.currentPhotoIndex < this.photoList.length - 1 ? '' : 'none';
        }
    }

    // ==================== Photo Loading ====================

    loadPhoto(photoId) {
        htmx.ajax('GET', `/photo/${photoId}/viewer`, {
            target: this.mediaContainer,
            swap: 'innerHTML'
        });
    }

    // ==================== Keyboard Handling ====================

    handleKeydown(e) {
        if (!this.isOpen) return;

        // Ignore if typing in input
        if (e.target.tagName === 'INPUT' || e.target.tagName === 'TEXTAREA') return;

        switch (e.key) {
            case 'Escape':
                if (this.infoPanelOpen) {
                    this.toggleInfoPanel(false);
                } else {
                    this.close();
                }
                e.preventDefault();
                break;
            case 'ArrowLeft':
                this.navigate(-1);
                e.preventDefault();
                break;
            case 'ArrowRight':
                this.navigate(1);
                e.preventDefault();
                break;
            case 'i':
                this.toggleInfoPanel();
                e.preventDefault();
                break;
            case 'f':
                this.toggleFavorite();
                e.preventDefault();
                break;
        }
    }

    // ==================== Actions ====================

    toggleFavorite() {
        document.getElementById('viewer-fav-btn')?.click();
    }

    toggleInfoPanel(forceState) {
        const newState = forceState !== undefined ? forceState : !this.infoPanelOpen;
        this.infoPanelOpen = newState;
        this.infoPanel.classList.toggle('open', newState);

        if (newState) {
            document.getElementById('viewer-info-btn')?.click();
        }
    }

    async sharePhoto() {
        const url = `${window.location.origin}/photo/${this.currentPhotoId}`;

        if (navigator.share) {
            try {
                await navigator.share({
                    title: 'Photo',
                    url: url
                });
                return;
            } catch (error) {
                if (error.name !== 'AbortError') {
                    console.error('Share failed:', error);
                }
            }
        }

        try {
            await navigator.clipboard.writeText(url);
            this.showToast('Link copied to clipboard');
        } catch (error) {
            console.error('Clipboard failed:', error);
            this.showToast('Failed to copy link', 'error');
        }
    }


    handlePhotoRemoval() {
        this.photoList = this.photoList.filter(p => p.id !== this.currentPhotoId);

        if (this.currentPhotoIndex >= this.photoList.length) {
            this.currentPhotoIndex = this.photoList.length - 1;
        }

        if (this.photoList.length > 0) {
            const next = this.photoList[this.currentPhotoIndex];
            this.currentPhotoId = next.id;
            this.thumbnailElement = next.element;
            this.loadPhoto(next.id);
            this.updateNavigation();
        } else {
            this.close();
        }
    }

    showToast(message, type = 'success') {
        const container = document.getElementById('errors-list');
        if (!container) return;

        const alert = document.createElement('div');
        alert.className = `alert alert-${type === 'error' ? 'error' : 'success'}`;
        alert.innerHTML = `<span>${message}</span>`;
        container.appendChild(alert);

        setTimeout(() => alert.remove(), 3000);
    }
}

// Initialize global instance with safe DOM ready check
let photoViewer;
if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', () => {
        photoViewer = new PhotoViewer();
    });
} else {
    photoViewer = new PhotoViewer();
}
