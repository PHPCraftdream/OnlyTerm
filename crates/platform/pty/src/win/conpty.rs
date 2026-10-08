use crate::cmdbuilder::CommandBuilder;
use crate::win::detach::{self, DetachedSession, ProcessIdentity, PtyProcessInfo};
use crate::win::pseudocon::PseudoCon;
use crate::{Child, MasterPty, PtyPair, PtySize, PtySystem, SlavePty};
use anyhow::Error;
use filedescriptor::{FileDescriptor, Pipe};
use std::io::Write;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::sync::{Arc, Mutex, Weak};
use winapi::um::wincon::COORD;

#[derive(Default)]
pub struct ConPtySystem {}

impl PtySystem for ConPtySystem {
    fn openpty(&self, size: PtySize) -> anyhow::Result<PtyPair> {
        let stdin = Pipe::new()?;
        let stdout = Pipe::new()?;

        let con = PseudoCon::new(
            COORD {
                X: size.cols as i16,
                Y: size.rows as i16,
            },
            stdin.read,
            stdout.write,
        )?;

        let master = ConPtyMasterPty {
            inner: Arc::new(Mutex::new(Inner {
                con,
                readable: stdout.read,
                writable: Some(stdin.write),
                size,
                input: Weak::new(),
                child_job: None,
                root_pid: None,
                detached: None,
                detaching: false,
                closing: false,
            })),
        };

        let slave = ConPtySlavePty {
            inner: master.inner.clone(),
        };

        Ok(PtyPair {
            master: Box::new(master),
            slave: Box::new(slave),
        })
    }
}

struct Inner {
    con: PseudoCon,
    readable: FileDescriptor,
    writable: Option<FileDescriptor>,
    size: PtySize,
    input: Weak<FileDescriptor>,
    child_job: Option<Weak<Mutex<super::JobState>>>,
    root_pid: Option<u32>,
    detached: Option<Arc<DetachedSession>>,
    detaching: bool,
    closing: bool,
}

impl Inner {
    pub fn resize(
        &mut self,
        num_rows: u16,
        num_cols: u16,
        pixel_width: u16,
        pixel_height: u16,
    ) -> Result<(), Error> {
        self.con.resize(COORD {
            X: num_cols as i16,
            Y: num_rows as i16,
        })?;
        self.size = PtySize {
            rows: num_rows,
            cols: num_cols,
            pixel_width,
            pixel_height,
        };
        Ok(())
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(session) = self.detached.as_ref() {
            if let Err(error) = session.close() {
                log::error!("Cannot signal detached PTY closure: {error:#}");
            }
        }
    }
}

struct ConPtyWriter {
    descriptor: Arc<FileDescriptor>,
}

impl Write for ConPtyWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        Write::write(&mut &*self.descriptor, bytes)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Write::flush(&mut &*self.descriptor)
    }
}

impl ConPtyMasterPty {
    pub fn child_processes(&self) -> anyhow::Result<Vec<PtyProcessInfo>> {
        let inner = self.inner.lock().unwrap();
        let job = if let Some(session) = inner.detached.as_ref() {
            session.root_job.try_clone()?
        } else {
            let state = inner
                .child_job
                .as_ref()
                .and_then(Weak::upgrade)
                .ok_or_else(|| anyhow::anyhow!("The pane process has already exited"))?;
            let state = state.lock().unwrap();
            detach::duplicate_kernel_handle(
                state
                    .handle
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("The pane is closing"))?
                    .as_raw_handle() as _,
            )?
        };
        let protected = inner
            .detached
            .as_ref()
            .map(|session| session.protected_job.as_raw_handle() as _);
        detach::processes(job.as_raw_handle() as _, protected)
    }

    pub fn detach_processes(
        &self,
        selected: &[ProcessIdentity],
        helper: &Path,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(!selected.is_empty(), "No processes selected");
        let (job, child_state, session, input, output, lifetime) = {
            let mut inner = self.inner.lock().unwrap();
            anyhow::ensure!(
                !inner.closing && !inner.detaching,
                "The pane is closing or already detaching processes"
            );
            let child_state = inner.child_job.as_ref().and_then(Weak::upgrade);
            let job = if let Some(session) = inner.detached.as_ref() {
                anyhow::ensure!(
                    session.is_alive()?,
                    "The detached-process keeper has exited"
                );
                session.root_job.try_clone()?
            } else {
                let state = child_state
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("The pane process has exited"))?;
                let state = state.lock().unwrap();
                detach::duplicate_kernel_handle(
                    state
                        .handle
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("The pane is closing"))?
                        .as_raw_handle() as _,
                )?
            };
            let input = inner
                .input
                .upgrade()
                .ok_or_else(|| anyhow::anyhow!("The pane input channel is closed"))?;
            let output = detach::duplicate_kernel_handle(inner.readable.as_raw_handle() as _)?;
            let lifetime = inner.con.lifetime_handles();
            let owned_lifetime = [
                detach::duplicate_kernel_handle(lifetime[0])?,
                detach::duplicate_kernel_handle(lifetime[1])?,
                detach::duplicate_kernel_handle(lifetime[2])?,
            ];
            inner.detaching = true;
            (
                job,
                child_state,
                inner.detached.as_ref().map(Arc::clone),
                input,
                output,
                owned_lifetime,
            )
        };
        // Spawn/handshake must not hold the PTY or child-killer mutex.
        let result = (|| -> anyhow::Result<()> {
            let processes = detach::selected_processes(job.as_raw_handle() as _, selected)?;
            let created = session.is_none();
            let session = match session {
                Some(session) => session,
                None => Arc::new(DetachedSession::launch(
                    helper,
                    [
                        lifetime[0].as_raw_handle() as _,
                        lifetime[1].as_raw_handle() as _,
                        lifetime[2].as_raw_handle() as _,
                    ],
                    input.as_raw_handle() as _,
                    output.as_raw_handle() as _,
                    job.as_raw_handle() as _,
                )?),
            };
            let mut inner = self.inner.lock().unwrap();
            let mut child_state = child_state.as_ref().map(|state| state.lock().unwrap());
            if inner.closing
                || child_state
                    .as_ref()
                    .is_some_and(|state| state.handle.is_none())
            {
                if created {
                    session.close()?;
                }
                anyhow::bail!("The pane closed before detachment completed");
            }
            let mut assigned = 0;
            let mut failures = vec![];
            for (identity, process) in processes {
                match detach::assign_protected(
                    process.as_raw_handle() as _,
                    session.protected_job.as_raw_handle() as _,
                ) {
                    Ok(()) => {
                        assigned += 1;
                        if inner.root_pid == Some(identity.pid) {
                            if let Some(state) = child_state.as_mut() {
                                state.preserve_root = true;
                            }
                        }
                    }
                    Err(error) => failures.push(format!("PID {}: {error:#}", identity.pid)),
                }
            }
            if assigned != 0 {
                inner.con.lifetime_transferred = true;
                inner.detached = Some(Arc::clone(&session));
                session.commit()?;
            } else if created {
                session.close()?;
            }
            if !failures.is_empty() {
                anyhow::bail!(
                    "Failed to detach {}. Successfully detached: {}",
                    failures.join("; "),
                    assigned
                );
            }
            Ok(())
        })();
        self.inner.lock().unwrap().detaching = false;
        result
    }

    pub fn close_detached_session(&self) -> bool {
        let mut inner = self.inner.lock().unwrap();
        inner.closing = true;
        if let Some(session) = inner.detached.as_ref() {
            if let Err(error) = session.close() {
                log::error!("Cannot signal detached PTY closure: {error:#}");
            }
            true
        } else {
            inner.detaching
        }
    }
}

#[derive(Clone)]
pub struct ConPtyMasterPty {
    inner: Arc<Mutex<Inner>>,
}

pub struct ConPtySlavePty {
    inner: Arc<Mutex<Inner>>,
}

impl MasterPty for ConPtyMasterPty {
    fn resize(&self, size: PtySize) -> anyhow::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        inner.resize(size.rows, size.cols, size.pixel_width, size.pixel_height)
    }

    fn get_size(&self) -> Result<PtySize, Error> {
        let inner = self.inner.lock().unwrap();
        Ok(inner.size)
    }

    fn try_clone_reader(&self) -> anyhow::Result<Box<dyn std::io::Read + Send>> {
        Ok(Box::new(self.inner.lock().unwrap().readable.try_clone()?))
    }

    fn take_writer(&self) -> anyhow::Result<Box<dyn std::io::Write + Send>> {
        let mut inner = self.inner.lock().unwrap();
        let descriptor = Arc::new(
            inner
                .writable
                .take()
                .ok_or_else(|| anyhow::anyhow!("writer already taken"))?,
        );
        inner.input = Arc::downgrade(&descriptor);
        Ok(Box::new(ConPtyWriter { descriptor }))
    }
}

impl SlavePty for ConPtySlavePty {
    fn spawn_command(&self, cmd: CommandBuilder) -> anyhow::Result<Box<dyn Child + Send + Sync>> {
        let mut inner = self.inner.lock().unwrap();
        let child = inner.con.spawn_command(cmd)?;
        inner.child_job = Some(Arc::downgrade(&child.job));
        inner.root_pid = child.process_id();
        Ok(Box::new(child))
    }
}
