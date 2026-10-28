// Unit tests of the pure helpers of the folders, sharing and destination picker code.
import { expect, test } from '@playwright/test';
import { matchesQuery, normalize } from '../../assets/js/folders/search.js';
import {
    DEFAULT_DESTINATION, destinationForFilter, selectionKind, typeLabel,
} from '../../assets/js/destination-picker/rules.js';
import { resolveDestination } from '../../assets/js/destination-picker/destination.js';
import { dateExpiry, presetExpiry } from '../../assets/js/sharing/expiry.js';
import { errorMessage } from '../../assets/js/core/api.js';

test.describe('folder search', () => {
    test('ignores accents and case', () => {
        expect(normalize('Vacanță')).toBe('vacanta');
        expect(normalize('ȘTEFAN în Ținut')).toBe('stefan in tinut');
        expect(matchesQuery('Vacanță 2024', 'vacanta')).toBe(true);
        expect(matchesQuery('Vacanta', 'VACANȚĂ')).toBe(true);
        expect(matchesQuery('Holiday', 'vacanta')).toBe(false);
    });

    test('an empty or blank query matches everything', () => {
        expect(matchesQuery('Holiday', '')).toBe(true);
        expect(matchesQuery('Holiday', '   ')).toBe(true);
    });
});

test.describe('destination picker rules', () => {
    const folder = { kind: 'folder', folderId: 3, name: 'Trip', isPublic: false, type: 'personal' };

    test('filter chips preselect like the Android app', () => {
        expect(destinationForFilter('family', folder)).toEqual({ kind: 'root', isPublic: true });
        expect(destinationForFilter('personal', folder)).toEqual({ kind: 'root', isPublic: false });
        expect(destinationForFilter('shared', folder)).toBeNull();
        expect(destinationForFilter('all', folder)).toBe(folder);
        expect(destinationForFilter('all', null)).toBeNull();
        expect(destinationForFilter('all', DEFAULT_DESTINATION)).toBe(DEFAULT_DESTINATION);
    });

    test('selection kind and type label', () => {
        expect(selectionKind(null)).toBe('none');
        expect(selectionKind(folder)).toBe('folder');
        expect(selectionKind({ kind: 'new', name: 'x', isPublic: true })).toBe('new');
        expect(typeLabel({ kind: 'new', name: 'x', isPublic: true })).toBe('Family');
        expect(typeLabel(folder)).toBe('Personal');
        expect(typeLabel({ ...folder, type: 'shared' })).toBe('Shared');
        expect(typeLabel({ ...folder, isPublic: true, type: 'family' })).toBe('Family');
    });

    test('resolveDestination for destinations that need no request', async () => {
        expect(await resolveDestination({ kind: 'root', isPublic: true })).toEqual({ makePublic: true });
        expect(await resolveDestination({ kind: 'root', isPublic: false })).toEqual({ makePublic: false });
        expect(await resolveDestination(folder)).toEqual({ folderId: 3 });
    });
});

test.describe('share expiry', () => {
    const now = Date.UTC(2026, 0, 31, 12, 0, 0);

    test('presets', () => {
        expect(presetExpiry('none', now)).toBeNull();
        expect(presetExpiry('day', now)).toBe(now / 1000 + 86400);
        expect(presetExpiry('week', now)).toBe(now / 1000 + 7 * 86400);
        // One calendar month: 31 Jan → 3 Mar (Feb has 28 days and the date overflows)
        expect(presetExpiry('month', now)).toBe(Date.UTC(2026, 2, 3, 12, 0, 0) / 1000);
        expect(() => presetExpiry('year', now)).toThrow();
    });

    test('a custom date expires at the end of that day (UTC)', () => {
        expect(dateExpiry('2026-10-12')).toBe(Date.UTC(2026, 9, 12, 23, 59, 59) / 1000);
        expect(dateExpiry('')).toBeNull();
        expect(dateExpiry('12/10/2026')).toBeNull();
        expect(dateExpiry('2026-13-45')).toBeNull();
    });
});

test('409 messages are shown to the user, like 400 ones', () => {
    expect(errorMessage(409, 'A folder with this name already exists')).toBe('A folder with this name already exists');
    expect(errorMessage(400, 'Bad name')).toBe('Bad name');
    expect(errorMessage(500, 'secret')).not.toBe('secret');
});
