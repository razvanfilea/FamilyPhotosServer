/**
 * Updates active tab state based on URL category parameter
 * Used by gallery pages with category tabs
 */
function updateActiveTabs() {
    const params = new URLSearchParams(window.location.search);
    const category = params.get('category') || 'all';
    document.querySelectorAll('.tabs .tab').forEach(tab => {
        const hxGet = tab.getAttribute('hx-get');
        if (hxGet) {
            const tabCategory = new URL(hxGet, window.location.origin).searchParams.get('category');
            tab.classList.toggle('tab-active', tabCategory === category);
        }
    });
}

// Listen for HTMX history events
document.addEventListener('htmx:pushedIntoHistory', updateActiveTabs);
window.addEventListener('popstate', updateActiveTabs);

// Also call after HTMX swaps that might contain tabs
document.addEventListener('htmx:afterSwap', function(e) {
    if (e.detail.target.id === 'photo-grid-container') {
        updateActiveTabs();
    }
});
