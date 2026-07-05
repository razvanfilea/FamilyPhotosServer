/// Controls which photos are accessible based on folder permission level.
#[derive(Debug, Clone, Copy)]
pub enum PhotoAccess {
    /// Own + public photos only (no shared folder access)
    Own = 0,
    /// Own + public + shared folders with any non-expired permission
    Read = 1,
    /// Own + public + shared folders with non-expired can_delete=true
    Delete = 2,
}
