// The E2E users, created by serve.mjs. carol owns no photos (shared with by some specs, and the seeded
// duplicates). Every spec that mutates a user's photos, favourites or trash has its own user so that
// specs running in parallel never share state: dave account (password change, which ends the user's other
// sessions for good), erin trash, frank Android API, grace and hank utilities, heidi selection, ivan upload,
// judy viewer.
export const USERS = Object.freeze([
    { id: 'alice', name: 'Alice', password: 'alice-password' },
    { id: 'bob', name: 'Bob', password: 'bob-password' },
    { id: 'carol', name: 'Carol', password: 'carol-password' },
    { id: 'dave', name: 'Dave', password: 'dave-password' },
    { id: 'erin', name: 'Erin', password: 'erin-password' },
    { id: 'frank', name: 'Frank', password: 'frank-password' },
    { id: 'grace', name: 'Grace', password: 'grace-password' },
    { id: 'hank', name: 'Hank', password: 'hank-password' },
    { id: 'heidi', name: 'Heidi', password: 'heidi-password' },
    { id: 'ivan', name: 'Ivan', password: 'ivan-password' },
    { id: 'judy', name: 'Judy', password: 'judy-password' },
]);
