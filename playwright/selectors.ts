/**
 * Centralized element selectors for Playwright tests.
 * Uses defensive fallback patterns to handle varying markup.
 */

// Photo elements
export const PHOTO_CARD = '[data-testid="photo-card"], .photo-card, [data-photo-id]';
export const PHOTO_GRID = '[data-testid="photo-grid"], .photo-grid, .grid';
export const PHOTO_MODAL = '[data-testid="photo-modal"], .modal, [role="dialog"]';
export const PHOTO_MODAL_CLOSE = '[data-testid="close-modal"], .modal-close, [aria-label="Close"]';

// Empty states
export const EMPTY_STATE = '[data-testid="empty-state"], .empty-state';
export const EMPTY_STATE_FAVORITES = '[data-testid="empty-state"], .empty-state, :text("No favorites")';
export const EMPTY_STATE_FOLDERS = '[data-testid="empty-state"], .empty-state, :text("No folders")';

// Pagination and infinite scroll
export const LOAD_MORE_TRIGGER = '[hx-trigger*="intersect"], [data-testid="load-more"]';
export const MONTH_HEADER = '[data-testid="month-header"], .month-header, h2:has-text("20")';

// Folders
export const FOLDER_LIST = '[data-testid="folder-list"], .folder-list, .folders';
export const FOLDER_CARD = '[data-testid="folder-card"], .folder-card, a[href*="/folder/"]';
export const FOLDER_LINK = 'a[href*="/folder/"]';

// Category tabs
export const CATEGORY_TAB_ALL = 'a[href="/?category=all"], a[href*="category=all"], [data-category="all"], :text("All")';
export const CATEGORY_TAB_PERSONAL = 'a[href*="category=personal"], [data-category="personal"], :text("Personal")';
export const CATEGORY_TAB_FAMILY = 'a[href*="category=family"], [data-category="family"], :text("Family")';
export const CATEGORY_LINK_PERSONAL = 'a[href*="category=personal"]';
export const CATEGORY_LINK_FAMILY = 'a[href*="category=family"]';

// Navigation
export const NAV_FAVORITES = 'a[href="/favorites"]';
export const NAV_FOLDERS = 'a[href="/folders"]';
export const NAV_TRASH = 'a[href="/trash"]';
export const NAV_GALLERY = 'a[href="/"], [data-testid="gallery-link"]';
export const NAV_LOGOUT = '[data-testid="logout"], a[href="/logout"], button:has-text("Logout")';

// Favorites
export const FAVORITE_BUTTON = '[data-testid="favorite-toggle"], button[hx-post*="favorite"], button[hx-delete*="favorite"], [aria-label*="favorite"]';

// Timeline
export const TIMELINE = '[data-testid="timeline"], .timeline, [id*="timeline"]';

// Login form
export const LOGIN_USER_ID = 'input[name="user_id"]';
export const LOGIN_PASSWORD = 'input[name="password"]';
export const LOGIN_SUBMIT = 'button[type="submit"]';
