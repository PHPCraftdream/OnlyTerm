//! Bounded process-wide refill executor for decoded image frames.

use onlyterm_blob_leases::BlobLease;
use std::io::Read;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, OnceLock};

const QUEUE_CAPACITY: usize = 16;
const JOB_LIMIT: usize = 16;
const WORKER_COUNT: usize = 2;
const TRANSIENT_BYTES: usize = 256 * 1024 * 1024;

struct Budget {
    used: Mutex<usize>,
    jobs: Mutex<usize>,
    limit: usize,
}

pub(super) struct Reservation {
    budget: Arc<Budget>,
    bytes: usize,
    jobs: usize,
}

impl Drop for Reservation {
    fn drop(&mut self) {
        let mut used = self.budget.used.lock().unwrap();
        *used -= self.bytes;
        let mut jobs = self.budget.jobs.lock().unwrap();
        *jobs -= self.jobs;
    }
}

pub(super) struct RefillResponse {
    pub(super) key: [u8; 32],
    pub(super) pixels: Option<Arc<Vec<u8>>>,
    pub(super) error: Option<String>,
    reservation: Reservation,
}

pub(super) struct RefillPixels {
    pub(super) pixels: Arc<Vec<u8>>,
    pub(super) reservation: Reservation,
}

pub(super) enum SubmitResult {
    Queued,
    Busy,
    Unavailable,
}

fn admission(
    used: usize,
    jobs: usize,
    expected: usize,
    byte_limit: usize,
    job_limit: usize,
) -> SubmitResult {
    if expected > byte_limit {
        return SubmitResult::Unavailable;
    }
    let exceeds_bytes = used
        .checked_add(expected)
        .is_none_or(|next| next > byte_limit);
    if exceeds_bytes || jobs >= job_limit {
        SubmitResult::Busy
    } else {
        SubmitResult::Queued
    }
}

impl RefillResponse {
    pub(super) fn finish(self) -> (Option<RefillPixels>, Option<String>) {
        let Self {
            pixels,
            error,
            reservation,
            ..
        } = self;
        (
            pixels.map(|pixels| RefillPixels {
                pixels,
                reservation,
            }),
            error,
        )
    }
}

struct Request {
    key: [u8; 32],
    lease: BlobLease,
    expected_len: usize,
    response_tx: SyncSender<RefillResponse>,
    reservation: Reservation,
}

struct Executor {
    tx: SyncSender<Request>,
    budget: Arc<Budget>,
}

static EXECUTOR: OnceLock<Executor> = OnceLock::new();

fn initialize() -> &'static Executor {
    EXECUTOR.get_or_init(|| {
        let (tx, rx) = sync_channel(QUEUE_CAPACITY);
        let rx = Arc::new(Mutex::new(rx));
        let executor = Executor {
            tx,
            budget: Arc::new(Budget {
                used: Mutex::new(0),
                jobs: Mutex::new(0),
                limit: TRANSIENT_BYTES,
            }),
        };
        for index in 0..WORKER_COUNT {
            let rx = Arc::clone(&rx);
            std::thread::Builder::new()
                .name(format!("image-refill-{index}"))
                .spawn(move || loop {
                    let request = rx.lock().unwrap().recv();
                    let Ok(request) = request else { break };
                    let response_tx = request.response_tx.clone();
                    let response = load_request(request);
                    let _ = response_tx.send(response);
                })
                .expect("spawn image refill worker");
        }
        executor
    })
}

pub(super) fn response_channel() -> (SyncSender<RefillResponse>, Receiver<RefillResponse>) {
    let _ = initialize();
    sync_channel(JOB_LIMIT)
}

pub(super) fn submit(
    key: [u8; 32],
    lease: BlobLease,
    expected_len: usize,
    response_tx: &SyncSender<RefillResponse>,
) -> SubmitResult {
    let executor = initialize();
    let mut used = executor.budget.used.lock().unwrap();
    let mut jobs = executor.budget.jobs.lock().unwrap();
    let status = admission(*used, *jobs, expected_len, executor.budget.limit, JOB_LIMIT);
    if !matches!(status, SubmitResult::Queued) {
        return status;
    }
    *used += expected_len;
    *jobs += 1;
    drop(used);
    drop(jobs);

    let reservation = Reservation {
        budget: Arc::clone(&executor.budget),
        bytes: expected_len,
        jobs: 1,
    };
    let request = Request {
        key,
        lease,
        expected_len,
        response_tx: response_tx.clone(),
        reservation,
    };
    match executor.tx.try_send(request) {
        Ok(()) => SubmitResult::Queued,
        Err(TrySendError::Full(request)) => {
            drop(request);
            SubmitResult::Busy
        }
        Err(TrySendError::Disconnected(request)) => {
            drop(request);
            SubmitResult::Unavailable
        }
    }
}

fn load_request(request: Request) -> RefillResponse {
    let result = (|| -> anyhow::Result<Arc<Vec<u8>>> {
        let mut reader = request.lease.get_reader()?;
        let data = read_exact_pixels(&mut reader, request.expected_len)?;
        Ok(Arc::new(data))
    })();
    match result {
        Ok(pixels) => RefillResponse {
            key: request.key,
            pixels: Some(pixels),
            error: None,
            reservation: request.reservation,
        },
        Err(err) => RefillResponse {
            key: request.key,
            pixels: None,
            error: Some(format!("{err:#}")),
            reservation: request.reservation,
        },
    }
}

fn read_exact_pixels<R: Read>(reader: &mut R, expected_len: usize) -> anyhow::Result<Vec<u8>> {
    let mut data = vec![0u8; expected_len];
    reader.read_exact(&mut data)?;
    let mut trailing = [0u8; 1];
    if reader.read(&mut trailing)? != 0 {
        anyhow::bail!("refill contains data beyond expected {expected_len} bytes");
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::{admission, Budget, SubmitResult};
    use std::io::Cursor;
    use std::sync::{Arc, Mutex};

    #[test]
    fn budget_releases_when_reservation_is_dropped() {
        let budget = Arc::new(Budget {
            used: Mutex::new(0),
            jobs: Mutex::new(1),
            limit: 8,
        });
        *budget.used.lock().unwrap() = 4;
        {
            let _reservation = super::Reservation {
                budget: Arc::clone(&budget),
                bytes: 4,
                jobs: 1,
            };
        }
        assert_eq!(*budget.used.lock().unwrap(), 0);
        assert_eq!(*budget.jobs.lock().unwrap(), 0);
    }

    #[test]
    fn admission_distinguishes_busy_budget_from_oversized_frame() {
        assert!(matches!(admission(6, 1, 4, 8, 2), SubmitResult::Busy));
        assert!(matches!(
            admission(0, 0, 9, 8, 2),
            SubmitResult::Unavailable
        ));
        assert!(matches!(admission(0, 2, 1, 8, 2), SubmitResult::Busy));
    }

    #[test]
    fn bounded_reader_rejects_trailing_data_without_growing() {
        let mut reader = Cursor::new(vec![1u8, 2, 3, 4, 5]);
        assert!(super::read_exact_pixels(&mut reader, 4).is_err());
    }
}
