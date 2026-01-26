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

    // Initial observation after DOM is ready
    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', observeImages);
    } else {
        observeImages();
    }

    // Re-observe after HTMX loads new content
    document.addEventListener('htmx:load', observeImages);
})();
