/**
 * Updates active segment state based on URL category parameter
 * Used by gallery pages with segmented button category selectors
 */
function updateActiveSegments() {
    const params = new URLSearchParams(window.location.search);
    const category = params.get('category') || 'all';
    document.querySelectorAll('.join .segment').forEach(segment => {
        const hxGet = segment.getAttribute('hx-get');
        if (hxGet) {
            const segmentCategory = new URL(hxGet, window.location.origin).searchParams.get('category');
            const isSelected = segmentCategory === category;
            segment.classList.toggle('btn-active', isSelected);
            segment.setAttribute('aria-pressed', isSelected ? 'true' : 'false');
        }
    });
}

// Listen for HTMX history events
document.addEventListener('htmx:pushedIntoHistory', updateActiveSegments);
window.addEventListener('popstate', updateActiveSegments);

// Also call after HTMX swaps that might change category state
document.addEventListener('htmx:afterSwap', function(e) {
    if (e.detail.target.id === 'photo-grid-container') {
        updateActiveSegments();
    }
});