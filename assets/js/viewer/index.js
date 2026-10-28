// The photo viewer, loaded by every page with the shell (templates/viewer/root.html). Other
// modules open it with `openViewer({photoId, contextKey})` or by emitting `fp:photo-activate`,
// and ask before deleting with `openDeleteDialog`.

export { closeViewer, openViewer } from './viewer.js';
export { openDeleteDialog } from './delete-dialog.js';
