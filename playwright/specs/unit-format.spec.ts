// Unit tests of the pure JS helpers, run by Playwright's test runner without a browser.
import { expect, test } from '@playwright/test';
import { formatBytes, formatDate, plural } from '../../assets/js/core/format.js';
import { parsePrefs, PREF_DEFAULTS, serializePrefs } from '../../assets/js/core/prefs.js';
import { eventCombo } from '../../assets/js/core/keyboard.js';
import { errorMessage } from '../../assets/js/core/api.js';
import { base64ToBytes } from '../../assets/js/vendor-wrappers/thumbhash.js';

test.describe('formatBytes', () => {
    test('uses 1024-based units like Android', () => {
        expect(formatBytes(0)).toBe('0 B');
        expect(formatBytes(1023)).toBe('1023 B');
        expect(formatBytes(1024)).toBe('1.0 KB');
        expect(formatBytes(512 * 1024)).toBe('512 KB');
        expect(formatBytes(1_258_291)).toBe('1.2 MB');
        expect(formatBytes(15 * 1024 * 1024)).toBe('15 MB');
        expect(formatBytes(10 * 1024 * 1024 - 1)).toBe('10 MB');
        expect(formatBytes(3 * 1024 ** 3)).toBe('3.0 GB');
    });
});

test('plural', () => {
    expect(plural(0, 'photo')).toBe('0 photos');
    expect(plural(1, 'photo')).toBe('1 photo');
    expect(plural(2, 'photo')).toBe('2 photos');
    expect(plural(2, 'person', 'people')).toBe('2 people');
});

test.describe('formatDate', () => {
    const date = Date.UTC(2024, 5, 8, 14, 3, 29) / 1000;

    test('formats every style in UTC', () => {
        expect(formatDate(date, 'monthHeader', Date.UTC(2024, 0, 1))).toBe('June');
        expect(formatDate(date, 'monthHeader', Date.UTC(2025, 0, 1))).toBe('June 2024');
        expect(formatDate(date, 'monthWithYear')).toBe('June 2024');
        expect(formatDate(date, 'viewer')).toBe('8 Jun 2024 · 14:03');
        expect(formatDate(date, 'long')).toBe('Saturday, 8 June 2024 · 14:03');
        expect(formatDate(date, 'accessible')).toBe('8 Jun 2024 14:03');
    });

    test('uses three-letter month abbreviations', () => {
        expect(formatDate(Date.UTC(2024, 8, 1) / 1000, 'viewer')).toBe('1 Sep 2024 · 00:00');
    });

    test('rejects unknown styles', () => {
        expect(() => formatDate(date, 'nope' as never)).toThrow();
    });
});

test.describe('prefs', () => {
    test('defaults when the cookie is missing or invalid', () => {
        expect(parsePrefs('')).toEqual(PREF_DEFAULTS);
        expect(parsePrefs('id=abc; other=1')).toEqual(PREF_DEFAULTS);
    });

    test('reads the fp_prefs cookie among others', () => {
        expect(parsePrefs('id=abc; fp_prefs=filter=family&zoom=3; x=1')).toEqual({
            ...PREF_DEFAULTS,
            filter: 'family',
            zoom: '3',
        });
    });

    test('serializes without defaults, round trip', () => {
        const prefs = { ...PREF_DEFAULTS, fsort: 'count_desc', fview: 'list' };
        const value = serializePrefs(prefs);
        expect(value).toBe('fsort=count_desc&fview=list');
        expect(parsePrefs(`fp_prefs=${value}`)).toEqual(prefs);
        expect(serializePrefs(PREF_DEFAULTS)).toBe('');
    });
});

test('keyboard combos', () => {
    const key = (k: string, mods: Partial<KeyboardEvent> = {}) =>
        eventCombo({ key: k, altKey: false, ctrlKey: false, metaKey: false, shiftKey: false, ...mods } as KeyboardEvent);

    expect(key('g')).toBe('g');
    expect(key('G', { shiftKey: true })).toBe('shift+g');
    expect(key('?', { shiftKey: true })).toBe('?');
    expect(key('+', { shiftKey: true })).toBe('+');
    expect(key('k', { ctrlKey: true, metaKey: true })).toBe('ctrl+meta+k');
    expect(key('ArrowLeft')).toBe('ArrowLeft');
});

test('error messages only expose 400 bodies', () => {
    expect(errorMessage(400, 'Folder name is too long')).toBe('Folder name is too long');
    expect(errorMessage(500, 'Database error: …')).toBe('genericError');
    expect(errorMessage(400, '')).toBe('genericError');
});

test('thumbhash base64 decoding', () => {
    expect([...base64ToBytes('AQID')]).toEqual([1, 2, 3]);
});
