// fetch wrapper for the JSON API: JSON in/out, 401 → login redirect, error toasts on failure.

import { toast } from './toast.js';
import { t } from './strings.js';

export class ApiError extends Error {
    constructor(status, message) {
        super(message);
        this.name = 'ApiError';
        this.status = status;
    }
}

/** Sends the browser to the login page, coming back to the current page afterwards. */
export function redirectToLogin() {
    const next = location.pathname + location.search;
    location.assign(next === '/' ? '/login' : `/login?next=${encodeURIComponent(next)}`);
}

/**
 * Error message for a failed response: the server's 400 and 409 bodies are user-facing, others
 * aren't.
 * @returns {string}
 */
export function errorMessage(status, body) {
    return (status === 400 || status === 409) && body ? body : t('genericError');
}

/**
 * @param {string} path
 * @param {{method?: string, json?: unknown, body?: BodyInit, query?: Record<string, unknown>, headers?: HeadersInit, signal?: AbortSignal, silent?: boolean}} options
 *   `silent` suppresses the error toasts (the error is still thrown).
 * @returns {Promise<any>} the parsed JSON body, the text body, or null for empty responses
 * @throws {ApiError} on HTTP errors (status 0 for network errors)
 */
export async function api(path, { method = 'GET', json, body, query, headers, signal, silent = false } = {}) {
    const url = new URL(path, location.origin);
    for (const [key, value] of Object.entries(query ?? {})) {
        if (value !== undefined && value !== null) url.searchParams.set(key, String(value));
    }

    const requestHeaders = new Headers(headers);
    requestHeaders.set('Accept', 'application/json');
    if (json !== undefined) {
        requestHeaders.set('Content-Type', 'application/json');
        body = JSON.stringify(json);
    }

    let response;
    try {
        response = await fetch(url, { method, body, headers: requestHeaders, signal, credentials: 'same-origin' });
    } catch (error) {
        if (error.name === 'AbortError') throw error;
        if (!silent) toast(t('networkError'), { type: 'error' });
        throw new ApiError(0, t('networkError'));
    }

    if (response.status === 401) {
        redirectToLogin();
        throw new ApiError(401, 'Unauthorized');
    }

    const text = await response.text();
    if (!response.ok) {
        const message = errorMessage(response.status, text);
        if (!silent) toast(message, { type: 'error' });
        throw new ApiError(response.status, message);
    }

    if (!text) return null;
    return response.headers.get('Content-Type')?.includes('application/json') ? JSON.parse(text) : text;
}

export const getJson = (path, options) => api(path, { ...options, method: 'GET' });
export const postJson = (path, json, options) => api(path, { ...options, method: 'POST', json });
