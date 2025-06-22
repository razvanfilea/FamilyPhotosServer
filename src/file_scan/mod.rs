use crate::file_scan::data_scan::DataScan;
use crate::http::{AppStateRef};
use tokio::task::JoinHandle;
use tracing::debug;

mod data_scan;
mod timestamp;

pub fn scan_new_files(app_state: AppStateRef) -> JoinHandle<()> {
    debug!("Started scanning for new files");
    DataScan::run(app_state)
}
