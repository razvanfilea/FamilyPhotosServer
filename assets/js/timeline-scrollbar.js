/**
 * Timeline Scrollbar - Google Photos-style scrollbar with year markers
 * Shows the full timeline from server data, with scroll position sync and click-to-jump
 */
class TimelineScrollbar {
    constructor() {
        // Don't initialize on mobile/touch devices
        if (this.isMobile()) {
            return;
        }

        this.scrollTarget = null;
        this.hideTimeout = null;
        this.isHovering = false;
        this.isDragging = false;
        this.lastScrollY = 0;

        // Month visibility tracking for accurate thumb positioning
        this.visibleMonthObserver = null;
        this.topVisibleMonth = null;
        this.monthVisibility = new Map();

        // Store bound event handlers for cleanup
        this.scrollTicking = false;
        this.boundOnScroll = () => {
            if (!this.scrollTicking) {
                requestAnimationFrame(() => {
                    this.onScroll();
                    this.scrollTicking = false;
                });
                this.scrollTicking = true;
            }
        };
        this.boundOnWheel = () => {
            if (this.scrollTarget) {
                this.cancelAutoScroll();
            }
        };
        this.boundHtmxAfterSwap = () => this.checkScrollTarget();
        this.boundHtmxLoad = () => {
            setTimeout(() => this.observeMonthHeaders(), 50);
        };
        this.boundOnDrag = (e) => this.onDrag(e);
        this.boundEndDrag = () => this.endDrag();

        this.createElements();
        this.bindEvents();
        this.updateTimeline();

        // Initialize month observer after DOM is ready
        requestAnimationFrame(() => {
            this.setupMonthObserver();
            this.updateThumbPosition();
        });
    }

    isMobile() {
        return window.matchMedia('(pointer: coarse)').matches || window.innerWidth <= 768;
    }

    createElements() {
        // Create container
        this.container = document.createElement('div');
        this.container.className = 'timeline-scrollbar';
        this.container.innerHTML = `
            <div class="timeline-scrollbar-track"></div>
            <div class="timeline-scrollbar-thumb"></div>
            <div class="timeline-tooltip"></div>
        `;
        document.body.appendChild(this.container);

        this.track = this.container.querySelector('.timeline-scrollbar-track');
        this.thumb = this.container.querySelector('.timeline-scrollbar-thumb');
        this.tooltip = this.container.querySelector('.timeline-tooltip');
        this.yearMarkers = [];
    }

    bindEvents() {
        // Scroll sync
        window.addEventListener('scroll', this.boundOnScroll, { passive: true });

        // Show on hover
        this.container.addEventListener('mouseenter', () => {
            this.isHovering = true;
            this.show();
        });

        this.container.addEventListener('mouseleave', () => {
            this.isHovering = false;
            if (!this.isDragging) {
                this.scheduleHide();
            }
        });

        // Thumb dragging
        this.thumb.addEventListener('mousedown', (e) => this.startDrag(e));
        document.addEventListener('mousemove', this.boundOnDrag);
        document.addEventListener('mouseup', this.boundEndDrag);

        // Cancel auto-scroll on manual scroll
        window.addEventListener('wheel', this.boundOnWheel, { passive: true });

        // HTMX integration for auto-scroll
        document.body.addEventListener('htmx:afterSwap', this.boundHtmxAfterSwap);

        // Re-observe month headers when new content is loaded
        document.body.addEventListener('htmx:load', this.boundHtmxLoad);
    }

    destroy() {
        // Disconnect intersection observer
        if (this.visibleMonthObserver) {
            this.visibleMonthObserver.disconnect();
            this.visibleMonthObserver = null;
        }

        // Clear any pending timeouts
        if (this.hideTimeout) {
            clearTimeout(this.hideTimeout);
            this.hideTimeout = null;
        }

        // Remove window event listeners
        window.removeEventListener('scroll', this.boundOnScroll);
        window.removeEventListener('wheel', this.boundOnWheel);

        // Remove document event listeners
        document.removeEventListener('mousemove', this.boundOnDrag);
        document.removeEventListener('mouseup', this.boundEndDrag);

        // Remove HTMX listeners
        document.body.removeEventListener('htmx:afterSwap', this.boundHtmxAfterSwap);
        document.body.removeEventListener('htmx:load', this.boundHtmxLoad);

        // Remove container from DOM
        if (this.container) {
            this.container.remove();
            this.container = null;
        }

        // Clear data references
        this.timelineData = null;
        this.monthVisibility.clear();
    }

    updateTimeline() {
        const data = window.TIMELINE_DATA || [];
        const total = window.TOTAL_PHOTOS || 0;

        this.timelineData = data;
        this.totalPhotos = total;

        // Clear existing year markers
        this.yearMarkers.forEach(m => m.remove());
        this.yearMarkers = [];

        if (data.length === 0) {
            this.container.classList.add('hidden');
            return;
        }
        this.container.classList.remove('hidden');

        // Render year markers
        this.renderYearMarkers();
    }

    renderYearMarkers() {
        const trackTop = 80; // top-20 = 5rem = 80px
        const trackBottom = window.innerHeight - 16; // bottom-4 = 1rem = 16px
        const trackHeight = trackBottom - trackTop;

        // Group month entries by year, only show first occurrence of each year
        const seenYears = new Set();
        const yearPositions = [];
        for (const entry of this.timelineData) {
            if (!seenYears.has(entry.year)) {
                seenYears.add(entry.year);
                const position = this.getPositionForEntry(entry, trackTop, trackHeight);
                yearPositions.push({ year: entry.year, position });
            }
        }

        // Spread out overlapping markers (minimum spacing)
        const MIN_SPACING = 20;
        for (let i = 1; i < yearPositions.length; i++) {
            const prev = yearPositions[i - 1];
            const curr = yearPositions[i];
            if (curr.position - prev.position < MIN_SPACING) {
                curr.position = prev.position + MIN_SPACING;
            }
        }

        // Create markers for all years
        yearPositions.forEach(({ year, position }) => {
            const marker = document.createElement('div');
            marker.className = 'timeline-year-marker';
            marker.textContent = year;
            marker.style.top = `${position}px`;
            marker.addEventListener('click', (e) => {
                e.stopPropagation();
                this.jumpToYear(year);
            });
            this.container.appendChild(marker);
            this.yearMarkers.push(marker);
        });
    }

    getPositionForEntry(entry, trackTop, trackHeight) {
        if (this.totalPhotos === 0) return trackTop;
        // Position based on cumulative count
        const progress = entry.cumulative_before / this.totalPhotos;
        return trackTop + (progress * trackHeight);
    }

    setupMonthObserver() {
        if (this.visibleMonthObserver) {
            this.visibleMonthObserver.disconnect();
        }

        this.visibleMonthObserver = new IntersectionObserver(
            (entries) => this.onMonthVisibilityChange(entries),
            {
                rootMargin: '-80px 0px -70% 0px',
                threshold: [0, 0.1, 0.5, 1.0]
            }
        );

        this.observeMonthHeaders();
    }

    observeMonthHeaders() {
        if (!this.visibleMonthObserver) return;
        document.querySelectorAll('.month-header[data-month]').forEach(header => {
            this.visibleMonthObserver.observe(header);
        });
    }

    onMonthVisibilityChange(entries) {
        for (const entry of entries) {
            const monthKey = entry.target.dataset.month;
            if (entry.isIntersecting) {
                this.monthVisibility.set(monthKey, entry.intersectionRatio);
            } else {
                this.monthVisibility.delete(monthKey);
            }
        }
        this.updateTopVisibleMonth();
    }

    updateTopVisibleMonth() {
        // Find the month header that is at or just above the top of the visible area
        const headers = document.querySelectorAll('.month-header[data-month]');
        const topThreshold = 100; // Consider headers within 100px of the top

        let bestMatch = null;
        let bestTop = -Infinity;

        for (const header of headers) {
            const top = header.getBoundingClientRect().top;
            // Find the header closest to (but at or above) the threshold
            if (top <= topThreshold && top > bestTop) {
                bestTop = top;
                bestMatch = header.dataset.month;
            }
        }

        // If no header is above the threshold, find the first one below it
        if (!bestMatch) {
            let closestBelow = null;
            let closestBelowTop = Infinity;
            for (const header of headers) {
                const top = header.getBoundingClientRect().top;
                if (top > topThreshold && top < closestBelowTop) {
                    closestBelowTop = top;
                    closestBelow = header.dataset.month;
                }
            }
            bestMatch = closestBelow;
        }

        this.topVisibleMonth = bestMatch;
    }

    /**
     * Get the year from a month key (e.g., "2024-06" -> 2024)
     */
    getYearFromMonthKey(monthKey) {
        if (!monthKey) return null;
        const parts = monthKey.split('-');
        return parts.length > 0 ? parseInt(parts[0], 10) : null;
    }

    /**
     * Format a month key (e.g., "2024-06") to a readable label (e.g., "June 2024")
     */
    formatMonthLabel(monthKey) {
        if (!monthKey) return null;
        const [yearStr, monthStr] = monthKey.split('-');
        const year = parseInt(yearStr, 10);
        const month = parseInt(monthStr, 10);

        const monthNames = [
            'January', 'February', 'March', 'April', 'May', 'June',
            'July', 'August', 'September', 'October', 'November', 'December'
        ];

        if (month >= 1 && month <= 12) {
            return `${monthNames[month - 1]} ${year}`;
        }
        return `${year}`;
    }

    getEntryAtPosition(y) {
        const trackTop = 80;
        const trackBottom = window.innerHeight - 16;
        const trackHeight = trackBottom - trackTop;

        const progress = Math.max(0, Math.min(1, (y - trackTop) / trackHeight));
        const targetCount = progress * this.totalPhotos;

        // Find the entry that contains this position
        for (let i = 0; i < this.timelineData.length; i++) {
            const entry = this.timelineData[i];
            const entryEnd = entry.cumulative_before + entry.count;
            if (targetCount >= entry.cumulative_before && targetCount < entryEnd) {
                return entry;
            }
        }
        return this.timelineData[this.timelineData.length - 1];
    }

    onScroll() {
        this.show();
        this.scheduleHide();
        this.updateThumbPosition();
        this.updateScrollTooltip();
    }

    updateThumbPosition() {
        this.updateTopVisibleMonth();

        const trackTop = 80;
        const trackBottom = window.innerHeight - 16;
        const trackHeight = trackBottom - trackTop;
        const thumbHeight = 48;

        let progress = 0;

        if (this.topVisibleMonth && this.timelineData?.length > 0) {
            // Find exact month entry using year_month field
            const entry = this.timelineData.find(e => e.year_month === this.topVisibleMonth);

            if (entry && this.totalPhotos > 0) {
                const baseProgress = entry.cumulative_before / this.totalPhotos;
                const monthProgress = this.getProgressWithinMonth(entry);
                const monthContribution = (entry.count / this.totalPhotos) * monthProgress;
                progress = Math.min(1, baseProgress + monthContribution);
            }
        } else {
            // Fallback to DOM scroll percentage
            const scrollTop = window.scrollY;
            const scrollHeight = document.documentElement.scrollHeight - window.innerHeight;
            progress = scrollHeight > 0 ? scrollTop / scrollHeight : 0;
        }

        const thumbTop = trackTop + (progress * (trackHeight - thumbHeight));
        this.thumb.style.top = `${thumbTop}px`;
        this.thumbCenterY = thumbTop + (thumbHeight / 2);
    }

    getProgressWithinMonth(entry) {
        // Find the month header for this specific month
        const header = document.querySelector(`.month-header[data-month="${entry.year_month}"]`);
        if (!header) return 0;

        const headerRect = header.getBoundingClientRect();

        // Find the next month's header
        const allHeaders = Array.from(document.querySelectorAll('.month-header[data-month]'));
        const currentIndex = allHeaders.findIndex(h => h.dataset.month === entry.year_month);
        const nextHeader = allHeaders[currentIndex + 1];

        let sectionHeight;
        if (nextHeader) {
            sectionHeight = nextHeader.getBoundingClientRect().top - headerRect.top;
        } else {
            const sentinel = document.querySelector('.load-more-sentinel');
            sectionHeight = sentinel
                ? sentinel.getBoundingClientRect().top - headerRect.top
                : document.documentElement.scrollHeight - headerRect.top - window.scrollY;
        }

        if (sectionHeight <= 0) return 0;
        const scrolledPastHeader = 80 - headerRect.top;
        return Math.max(0, Math.min(1, scrolledPastHeader / sectionHeight));
    }

    updateScrollTooltip() {
        if (this.isDragging || !this.timelineData?.length) return;

        // Use the visible month from DOM for accurate display
        if (this.topVisibleMonth) {
            // Validate month exists in timeline data
            const validMonth = this.timelineData.some(e => e.year_month === this.topVisibleMonth);
            if (validMonth) {
                const label = this.formatMonthLabel(this.topVisibleMonth);
                if (label) {
                    this.tooltip.textContent = label;
                    this.tooltip.style.top = `${this.thumbCenterY}px`;
                    this.tooltip.classList.add('visible');
                    return;
                }
            }
        }

        // Fallback to month label from timeline data
        const entry = this.getEntryAtPosition(this.thumbCenterY);
        if (entry) {
            const label = this.formatMonthLabel(entry.year_month);
            this.tooltip.textContent = label || entry.year;
            this.tooltip.style.top = `${this.thumbCenterY}px`;
            this.tooltip.classList.add('visible');
        }
    }

    hideTooltip() {
        this.tooltip.classList.remove('visible');
    }

    jumpToYear(year) {
        // Find first month-header of this year
        const target = document.querySelector(`.month-header[data-month^="${year}-"]`);
        if (target) {
            // Check if the target is in a virtualized page
            const page = target.closest('.photo-page');
            if (page && page.classList.contains('virtualized')) {
                // Wait for page to materialize then scroll
                this.scrollTarget = year;
                this.showLoadingIndicator(year);
                // Watch for the target to appear
                this.watchForTarget(year);
            } else {
                // Target is in DOM and visible, scroll to it
                target.scrollIntoView({ behavior: 'smooth', block: 'start' });
                this.scrollTarget = null;
                this.hideLoadingIndicator();
            }
        } else {
            // Target not loaded, start auto-scroll
            this.scrollTarget = year;
            this.showLoadingIndicator(year);
            this.autoScrollToLoadMore();
        }
    }

    watchForTarget(year) {
        const checkTarget = () => {
            const target = document.querySelector(`.month-header[data-month^="${year}-"]`);
            if (target) {
                const page = target.closest('.photo-page');
                if (!page || !page.classList.contains('virtualized')) {
                    // Target is now available
                    setTimeout(() => {
                        target.scrollIntoView({ behavior: 'smooth', block: 'start' });
                    }, 100);
                    this.scrollTarget = null;
                    this.hideLoadingIndicator();
                    return;
                }
            }
            // Keep checking if we still have a scroll target
            if (this.scrollTarget === year) {
                setTimeout(checkTarget, 100);
            }
        };
        checkTarget();
    }

    autoScrollToLoadMore() {
        if (!this.scrollTarget) return;

        // Scroll to bottom to trigger infinite scroll
        window.scrollTo({
            top: document.documentElement.scrollHeight,
            behavior: 'smooth'
        });
    }

    checkScrollTarget() {
        if (!this.scrollTarget) return;

        const year = this.scrollTarget;
        const target = document.querySelector(`.month-header[data-month^="${year}-"]`);
        if (target) {
            // Found the target, scroll to it
            setTimeout(() => {
                target.scrollIntoView({ behavior: 'smooth', block: 'start' });
            }, 100);
            this.scrollTarget = null;
            this.hideLoadingIndicator();
        } else {
            // Check if there's more content to load
            const sentinel = document.querySelector('.load-more-sentinel');
            if (sentinel) {
                // Continue auto-scrolling
                setTimeout(() => this.autoScrollToLoadMore(), 200);
            } else {
                // No more content, cancel
                this.cancelAutoScroll();
            }
        }
    }

    cancelAutoScroll() {
        this.scrollTarget = null;
        this.hideLoadingIndicator();
    }

    showLoadingIndicator(year) {
        if (!this.loadingIndicator) {
            this.loadingIndicator = document.createElement('div');
            this.loadingIndicator.className = 'timeline-loading-indicator';
            this.container.appendChild(this.loadingIndicator);
        }
        this.loadingIndicator.textContent = `Loading ${year}...`;
        this.loadingIndicator.style.display = 'block';
    }

    hideLoadingIndicator() {
        if (this.loadingIndicator) {
            this.loadingIndicator.style.display = 'none';
        }
    }

    startDrag(e) {
        e.preventDefault();
        this.isDragging = true;
        this.container.classList.add('visible');
        document.body.style.userSelect = 'none';
    }

    onDrag(e) {
        if (!this.isDragging) return;

        const trackTop = 80;
        const trackBottom = window.innerHeight - 16;
        const trackHeight = trackBottom - trackTop;
        const thumbHeight = 48;

        const y = Math.max(trackTop, Math.min(trackBottom - thumbHeight, e.clientY));
        const progress = (y - trackTop) / (trackHeight - thumbHeight);

        const scrollHeight = document.documentElement.scrollHeight - window.innerHeight;
        window.scrollTo(0, progress * scrollHeight);

        // Update tooltip while dragging
        const entry = this.getEntryAtPosition(e.clientY);
        if (entry) {
            const label = this.formatMonthLabel(entry.year_month);
            this.tooltip.textContent = label || entry.year;
            this.tooltip.style.top = `${e.clientY}px`;
            this.tooltip.classList.add('visible');
        }
    }

    endDrag() {
        if (!this.isDragging) return;
        this.isDragging = false;
        document.body.style.userSelect = '';
        this.hideTooltip();
        if (!this.isHovering) {
            this.scheduleHide();
        }
    }

    show() {
        this.container.classList.add('visible');
        if (this.hideTimeout) {
            clearTimeout(this.hideTimeout);
            this.hideTimeout = null;
        }
    }

    scheduleHide() {
        if (this.hideTimeout) {
            clearTimeout(this.hideTimeout);
        }
        this.hideTimeout = setTimeout(() => {
            if (!this.isHovering && !this.isDragging) {
                this.container.classList.remove('visible');
                this.hideTooltip();
            }
        }, 1500);
    }
}

// Initialize on page load, destroying previous instance if it exists
document.addEventListener('DOMContentLoaded', () => {
    if (window.timelineScrollbar) {
        window.timelineScrollbar.destroy();
    }
    window.timelineScrollbar = new TimelineScrollbar();
});

// Cleanup before HTMX page swap (before new page loads)
document.body.addEventListener('htmx:beforeSwap', (e) => {
    // Only destroy on full page swaps (not partial swaps)
    if (e.detail.target === document.body || e.detail.target.id === 'main-content') {
        if (window.timelineScrollbar) {
            window.timelineScrollbar.destroy();
            window.timelineScrollbar = null;
        }
    }
});

// Cleanup on page unload
window.addEventListener('beforeunload', () => {
    if (window.timelineScrollbar) {
        window.timelineScrollbar.destroy();
        window.timelineScrollbar = null;
    }
});
