// The E2E users, created by serve.mjs. carol owns no photos. dave is reserved for the account test
// that changes a password (it ends the user's other sessions for good). erin is reserved for the trash
// actions (Restore all, Empty trash), which need a trash that no other spec fills.
export const USERS = Object.freeze([
    { id: 'alice', name: 'Alice', password: 'alice-password' },
    { id: 'bob', name: 'Bob', password: 'bob-password' },
    { id: 'carol', name: 'Carol', password: 'carol-password' },
    { id: 'dave', name: 'Dave', password: 'dave-password' },
    { id: 'erin', name: 'Erin', password: 'erin-password' },
]);
