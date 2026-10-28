// The E2E users, created by serve.mjs. carol owns no photos and is reserved for destructive
// account tests (e.g. changing the password).
export const USERS = Object.freeze([
    { id: 'alice', name: 'Alice', password: 'alice-password' },
    { id: 'bob', name: 'Bob', password: 'bob-password' },
    { id: 'carol', name: 'Carol', password: 'carol-password' },
]);
