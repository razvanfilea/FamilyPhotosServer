import { expect, gotoReady, loginAs, test, user } from '../fixtures/auth';

async function fillLogin(page: import('@playwright/test').Page, username: string, password: string) {
    await page.getByTestId('login-username').fill(username);
    await page.getByTestId('login-password').fill(password);
}

test.describe('login page', () => {
    test('logs in, normalizing the username, and lands on the timeline', async ({ page }) => {
        await gotoReady(page, '/login');
        await expect(page.getByRole('heading', { name: 'Family Photos' })).toBeVisible();
        await expect(page.getByText("Your family's moments, kept at home.")).toBeVisible();
        await expect(page.getByRole('heading', { name: 'Sign in' })).toBeVisible();

        const submit = page.getByTestId('login-submit');
        await expect(submit).toBeDisabled();
        await page.getByTestId('login-username').fill('  Alice ');
        await expect(submit).toBeDisabled();
        await page.getByTestId('login-password').fill(user('alice').password);
        await expect(submit).toBeEnabled();

        await submit.click();
        await expect(page).toHaveURL('/');
        await expect(page.getByTestId('user-menu-name')).toHaveText('Alice');
    });

    test('shows an error for a wrong password', async ({ page }) => {
        await gotoReady(page, '/login');
        await fillLogin(page, 'alice', 'wrong-password');
        await page.getByTestId('login-submit').click();

        await expect(page.getByTestId('login-error')).toHaveText('Invalid username or password.');
        await expect(page).toHaveURL('/login');
    });

    test('shows an error when the server is unreachable', async ({ page }) => {
        await gotoReady(page, '/login');
        await page.route('**/api/login', (route) => route.abort('connectionrefused'));
        await fillLogin(page, 'alice', user('alice').password);
        await page.getByTestId('login-submit').click();

        await expect(page.getByTestId('login-error')).toHaveText('Cannot connect to the server.');
        await expect(page.getByTestId('toast')).toHaveCount(0);
    });

    test('toggles the password visibility', async ({ page }) => {
        await gotoReady(page, '/login');
        const password = page.getByTestId('login-password');
        const toggle = page.getByTestId('login-password-toggle');

        await expect(password).toHaveAttribute('type', 'password');
        await toggle.click();
        await expect(password).toHaveAttribute('type', 'text');
        await expect(toggle).toHaveAttribute('aria-pressed', 'true');
        await expect(toggle).toHaveAccessibleName('Hide password');
        await toggle.click();
        await expect(password).toHaveAttribute('type', 'password');
    });

    test('honours next after logging in', async ({ page }) => {
        await gotoReady(page, '/folders?q=trip');
        await expect(page).toHaveURL('/login?next=%2Ffolders%3Fq%3Dtrip');

        await fillLogin(page, 'bob', user('bob').password);
        await page.getByTestId('login-submit').click();
        await expect(page).toHaveURL('/folders?q=trip');
    });

    test('ignores next when it points to another origin', async ({ page }) => {
        await gotoReady(page, '/login?next=//example.com/steal');
        await fillLogin(page, 'bob', user('bob').password);
        await page.getByTestId('login-submit').click();
        await expect(page).toHaveURL('/');
    });

    test('redirects to the timeline when already logged in', async ({ alicePage }) => {
        await alicePage.goto('/login');
        await expect(alicePage).toHaveURL('/');
    });
});

test.describe('protected routes', () => {
    test('pages redirect to the login page', async ({ page }) => {
        await page.goto('/utilities');
        await expect(page).toHaveURL('/login?next=%2Futilities');
        await expect(page.getByTestId('login-form')).toBeVisible();
    });

    test('fragments answer 401 with HX-Redirect back to the current page', async ({ request, baseURL }) => {
        const response = await request.get('/ui/dialogs/shortcuts', {
            headers: { 'HX-Request': 'true', 'HX-Current-URL': `${baseURL}/folders?q=a` },
            maxRedirects: 0,
        });
        expect(response.status()).toBe(401);
        expect(response.headers()['hx-redirect']).toBe('/login?next=%2Ffolders%3Fq%3Da');
    });

    test('an HTMX request with an expired session goes to the login page', async ({ page }) => {
        await loginAs(page, 'alice');
        await gotoReady(page, '/folders');
        await page.context().clearCookies();

        await page.getByTestId('user-menu-button').click();
        await page.getByTestId('user-menu-shortcuts').click();
        await expect(page).toHaveURL('/login?next=%2Ffolders');
    });

    test('the API answers 401 (Android compatibility)', async ({ request }) => {
        const response = await request.get('/api/profile');
        expect(response.status()).toBe(401);
    });
});

test.describe('logout', () => {
    test('logging out ends the session', async ({ page }) => {
        await loginAs(page, 'alice');
        await page.goto('/');
        await expect(page).toHaveURL('/');

        const response = await page.request.post('/api/logout', { headers: { Accept: 'application/json' } });
        expect(await response.json()).toEqual({ message: 'Logged out' });

        await page.goto('/settings');
        await expect(page).toHaveURL('/login?next=%2Fsettings');
    });

    test('the API still returns JSON for the Android app', async ({ page }) => {
        const response = await page.request.post('/api/login', {
            form: { user_id: 'bob', password: user('bob').password },
        });
        expect(await response.json()).toEqual({ user_id: 'bob', display_name: 'Bob' });

        const wrong = await page.request.post('/api/login', { form: { user_id: 'bob', password: 'nope' } });
        expect(wrong.status()).toBe(401);
    });
});
