// Lazy load images
(function() {
    const observer = new IntersectionObserver((entries) => {
        entries.forEach(entry => {
            if (entry.isIntersecting) {
                const img = entry.target;
                if (img.dataset.src) {
                    img.src = img.dataset.src;
                    delete img.dataset.src;
                }
                observer.unobserve(img);
            }
        });
    }, {
        rootMargin: '200px 0px'
    });

    function observeImages() {
        document.querySelectorAll('img[data-src]:not([data-observed])').forEach(img => {
            img.dataset.observed = 'true';
            observer.observe(img);
        });
    }

    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', observeImages);
    } else {
        observeImages();
    }

    document.addEventListener('htmx:load', observeImages);
})();

// Infinite scroll preloading - trigger load before sentinel is visible
(function() {
    const ROOT_MARGIN_PX = 2000;

    function observeSentinel(sentinel, immediate) {
        if (sentinel.dataset.observed) return;
        sentinel.dataset.observed = 'true';

        if (immediate) {
            htmx.trigger(sentinel, 'prefetch');
            return;
        }

        const observer = new IntersectionObserver((entries) => {
            if (entries[0].isIntersecting) {
                observer.disconnect();
                htmx.trigger(sentinel, 'prefetch');
            }
        }, { rootMargin: `0px 0px ${ROOT_MARGIN_PX}px 0px` });
        observer.observe(sentinel);
    }

    function observeSentinels(immediate) {
        document.querySelectorAll('.load-more-sentinel').forEach(s => observeSentinel(s, immediate));
    }

    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', () => observeSentinels(true));
    } else {
        observeSentinels(true);
    }

    document.body.addEventListener('htmx:load', () => observeSentinels(false));
})();
